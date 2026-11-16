namespace Tungsten.Core;

public sealed class EditorSession
{
    private readonly object sync = new();
    private readonly SemaphoreSlim operations = new(1);
    private readonly SemaphoreSlim persistence = new(1);
    private readonly INoteService service;
    private readonly IDraftStore store;
    private Draft current;
    private Draft? durable;
    private bool retired;
    public event EventHandler? RemoteChanged;
    private bool IsRetired { get { lock (sync) return retired; } }
    public EditorSession(Draft draft, INoteService service, IDraftStore store)
    {
        current = draft.Validate();
        if (draft.Server != service.Origin.AbsoluteUri) throw new InvalidDataException("Draft belongs to another server.");
        this.service = service; this.store = store;
        if (draft.Pending is not null && draft.Block == SaveBlock.None) current = draft with { Block = SaveBlock.Uncertain };
    }
    public Draft Current { get { lock (sync) return current; } }
    public SaveBlock Block => Current.Block;
    public bool IsDurable { get { lock (sync) return current == durable; } }
    public bool IsSaved => SavePolicy.IsSaved(Current);
    public void Edit(string body)
    {
        Rules.LocalBody(body);
        lock (sync) current = current with { Body = body };
    }
    private async Task<bool> PersistAsync()
    {
        await persistence.WaitAsync();
        try
        {
            return !IsRetired && await WriteCurrentAsync();
        }
        finally { persistence.Release(); }
    }
    private async Task<bool> WriteCurrentAsync()
    {
        var snapshot = Current;
        await store.WriteAsync(snapshot);
        lock (sync) { durable = snapshot; return current == snapshot; }
    }
    public Task<bool> CheckpointAsync() => PersistAsync();
    public async Task<bool> RetireAsync(Func<Task> commit, Func<bool> canRetire, Action publish)
    {
        await operations.WaitAsync();
        try
        {
            await persistence.WaitAsync();
            try
            {
                if (IsRetired || !canRetire()) return false;
                await commit();
                if (!canRetire()) { await WriteCurrentAsync(); return false; }
                lock (sync) retired = true;
                publish(); return true;
            }
            finally { persistence.Release(); }
        }
        finally { operations.Release(); }
    }
    public async Task SaveAsync(Func<bool>? canSend = null)
    {
        await operations.WaitAsync();
        try
        {
            if (IsRetired || canSend?.Invoke() == false || !SavePolicy.CanSend(Current)) return;
            await PersistAsync();
            if (canSend?.Invoke() == false) return;
            Draft? outgoing = null;
            lock (sync)
            {
                if (SavePolicy.CanSend(current))
                {
                    Rules.Body(current.Body);
                    current = SavePolicy.Begin(current);
                    outgoing = current;
                }
            }
            if (outgoing is null) { await PersistAsync(); return; }
            try
            {
                await PersistAsync();
                if (canSend?.Invoke() == false)
                {
                    lock (sync) current = current with { Pending = null };
                    await PersistAsync(); return;
                }
                var note = await service.SaveAsync(outgoing);
                lock (sync) current = SavePolicy.Acknowledge(current, outgoing, note);
                await PersistAsync();
            }
            catch (Exception error) when (error is IOException or InvalidDataException or HttpRequestException or OperationCanceledException or UnauthorizedAccessException)
            {
                var blocked = error is RemoteException remote ? remote.Status switch
                {
                    409 or 412 => SaveBlock.Conflict,
                    404 => SaveBlock.Missing,
                    _ => SaveBlock.Uncertain
                } : SaveBlock.Uncertain;
                lock (sync) current = current with { Block = blocked };
                throw;
            }
        }
        finally { operations.Release(); }
    }
    public async Task RetryAsync()
    {
        await operations.WaitAsync();
        try
        {
            if (IsRetired) return;
            var snapshot = Current;
            Note? remote;
            try { remote = (await service.ReadAsync(snapshot.Name)).Validate(snapshot.Name); }
            catch (RemoteException error) when (error.Status == 404) { remote = null; }
            lock (sync)
            {
                if (remote is not null && remote.Body == snapshot.Pending?.Body)
                    current = current with { Revision = remote.Revision, BaseBody = remote.Body, Pending = null };
                else if (remote is not null && remote.Revision == snapshot.Revision)
                    current = current with { Pending = null };
                else if (remote is null && snapshot.Revision is null)
                    current = current with { Pending = null };
                else { current = current with { Block = remote is null ? SaveBlock.Missing : SaveBlock.Conflict }; return; }
            }
            await PersistAsync();
            lock (sync) current = current with { Block = SaveBlock.None };
            await PersistAsync();
        }
        finally { operations.Release(); }
    }
    public async Task RefreshAsync(Func<bool>? canApply = null)
    {
        if (!await operations.WaitAsync(0)) return;
        try
        {
            var snapshot = Current;
            if (IsRetired || snapshot.Revision is null || Block != SaveBlock.None || snapshot.Pending is not null) return;
            Note remote;
            try { remote = (await service.ReadAsync(snapshot.Name)).Validate(snapshot.Name); }
            catch (RemoteException error) when (error.Status == 404)
            { lock (sync) current = current with { Block = SaveBlock.Missing }; RemoteChanged?.Invoke(this, EventArgs.Empty); await PersistAsync(); return; }
            if (canApply?.Invoke() == false || remote.Revision == snapshot.Revision) return;
            lock (sync)
            {
                current = SavePolicy.Observe(current, snapshot, remote);
            }
            RemoteChanged?.Invoke(this, EventArgs.Empty);
            await PersistAsync();
        }
        finally { operations.Release(); }
    }
}
