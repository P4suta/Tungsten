using CommunityToolkit.Mvvm.ComponentModel;

namespace Tungsten.Core;

public sealed class CompositionInProgressException() : InvalidOperationException("Finish text composition before continuing.");

public sealed class ClientModel(IDraftRepository store, Func<Uri, NoteClient>? factory = null, Func<bool>? canLeave = null) : ObservableObject
{
    private readonly SemaphoreSlim navigation = new(1);
    private string body = "";
    private string problem = "";
    public string Body
    {
        get => body;
        set
        {
            if (body == value) return;
            Editor?.Edit(value);
            SetProperty(ref body, value);
            NotifyEditor();
        }
    }
    public string Problem { get => problem; private set => SetProperty(ref problem, value); }
    public string Title => Editor is null ? "Select a note or create a note" : Rules.Title(Body);
    public string Status => Editor is null ? "" : Editor.Block switch
    {
        SaveBlock.Conflict => "Conflict: your draft is kept on this device.",
        SaveBlock.Missing => "The remote note was deleted. Your draft is kept on this device.",
        SaveBlock.Uncertain => "Delivery is uncertain. Check the server before retrying.",
        _ => Editor.IsSaved ? "Saved" : Editor.IsDurable ? "Draft on this device" : "Storing draft…"
    };
    public bool HasEditor => Editor is not null;
    public bool HasConnection => Client is not null;
    public bool HasSavedNote => Editor?.Current.Revision is not null;
    public bool CanCloseNow() => CanLeaveNow() && (Editor?.IsDurable ?? true);
    public EditorSession? Editor { get; private set; }
    public NoteClient? Client { get; private set; }
    public Workspace[] Workspaces { get; private set; } = [];
    private NoteClient Connected => Client ?? throw new InvalidOperationException("Connect to a server first.");
    public async Task RefreshWorkspacesAsync()
    {
        List<Workspace> rows = []; HashSet<string> identities = []; string? after = null;
        do
        {
            var page = await Connected.WorkspacesAsync(after);
            foreach (var workspace in page.Workspaces)
            { if (!identities.Add(workspace.Id)) throw new InvalidDataException("Repeated workspace identity."); rows.Add(workspace); }
            after = page.Next;
        } while (after is not null);
        Workspaces = rows.ToArray(); OnPropertyChanged(nameof(Workspaces));
    }
    private void UpdateWorkspace(Workspace workspace)
    {
        Workspaces = Workspaces.Where(w => w.Id != workspace.Id).Append(workspace).OrderBy(w => long.Parse(w.Id, System.Globalization.CultureInfo.InvariantCulture)).ToArray();
        OnPropertyChanged(nameof(Workspaces));
    }
    public async Task CreateWorkspaceAsync(string title) => UpdateWorkspace(await Connected.CreateWorkspaceAsync(title));
    public async Task RenameWorkspaceAsync(string id, string title)
        => UpdateWorkspace(await Connected.RenameWorkspaceAsync(Workspaces.Single(w => w.Id == id), title));
    public async Task DeleteWorkspaceAsync(string id)
    {
        await Connected.DeleteWorkspaceAsync(Workspaces.Single(w => w.Id == id));
        Workspaces = Workspaces.Where(w => w.Id != id).ToArray(); OnPropertyChanged(nameof(Workspaces));
    }
    public async Task MembershipAsync(string id, string name, bool included)
        => UpdateWorkspace(await Connected.MembershipAsync(id, name, included));
    private void NotifyEditor()
    { OnPropertyChanged(nameof(Title)); OnPropertyChanged(nameof(Status)); OnPropertyChanged(nameof(HasEditor)); OnPropertyChanged(nameof(HasConnection)); OnPropertyChanged(nameof(HasSavedNote)); }
    private void Select(Draft draft)
    {
        if (!CanLeaveNow()) throw new CompositionInProgressException();
        Editor = new(draft, Client ?? throw new InvalidOperationException("Connect to a server first."), store);
        body = draft.Body; OnPropertyChanged(nameof(Body)); NotifyEditor();
    }
    private async Task SelectAsync(Draft draft)
    {
        await store.WriteAsync(draft);
        if (!await CanLeaveAsync()) throw LeavingError();
        Select(draft);
    }
    public async Task ConnectAsync(Uri server)
    {
        await navigation.WaitAsync();
        try
        {
            if (!await CanLeaveAsync()) throw LeavingError();
            var client = factory?.Invoke(server) ?? new NoteClient(server);
            Client?.Dispose(); Client = client; Editor = null; body = ""; Workspaces = [];
            OnPropertyChanged(nameof(Workspaces));
            OnPropertyChanged(nameof(Body)); NotifyEditor();
        }
        finally { navigation.Release(); }
    }
    public async Task NewAsync()
    {
        await navigation.WaitAsync();
        try
        {
            if (!await CanLeaveAsync()) throw LeavingError();
            var draft = Draft.New(Client?.Origin ?? throw new InvalidOperationException("Connect to a server first."));
            await SelectAsync(draft);
        }
        finally { navigation.Release(); }
    }
    public async Task OpenAsync(string name)
    {
        await navigation.WaitAsync();
        try
        {
            if (!await CanLeaveAsync()) throw LeavingError();
            if (Editor?.Current.Name == name) return;
            var client = Client ?? throw new InvalidOperationException("Connect to a server first.");
            var recovery = await store.LoadAsync(client.Origin);
            var draft = recovery.Drafts.SingleOrDefault(d => d.Name == name)
                ?? Draft.From(client.Origin, await client.ReadAsync(name));
            if (!await CanLeaveAsync()) throw LeavingError();
            await SelectAsync(draft);
        }
        finally { navigation.Release(); }
    }
    public async Task SaveAsync()
    {
        if (Editor is not { } editor) return;
        try { await editor.SaveAsync(canLeave); Problem = ""; }
        catch (Exception error) when (error is IOException or InvalidDataException or HttpRequestException or OperationCanceledException)
        { Problem = error.Message; throw; }
        finally { NotifyEditor(); }
    }
    public async Task AutosaveAsync(bool composing, bool busy, Func<Task> send)
    {
        if (composing || Editor is not { IsSaved: false } editor) return;
        if (!await CanLeaveAsync()) return;
        if (!busy && ReferenceEquals(editor, Editor) && editor.Block == SaveBlock.None) await send();
        NotifyEditor();
    }
    public async Task<bool> CanLeaveAsync()
    {
        if (Editor is not { } editor) return CanLeaveNow();
        try
        {
            var saved = await editor.CheckpointAsync();
            if (!CanLeaveNow()) return false;
            Problem = saved ? "" : "Writing changed while the draft was being stored. Try again.";
            return saved;
        }
        catch (Exception error) when (error is IOException or InvalidDataException or UnauthorizedAccessException)
        { Problem = "The draft could not be stored. Keep this window open. " + error.Message; return false; }
    }
    private bool CanLeaveNow() => canLeave?.Invoke() != false;
    private Exception LeavingError() => CanLeaveNow() ? new IOException(Problem) : new CompositionInProgressException();
    public async Task SaveAsNewAsync()
    {
        await navigation.WaitAsync();
        try
        {
            if (!await CanLeaveAsync()) throw LeavingError();
            var draft = Draft.New(Client?.Origin ?? throw new InvalidOperationException("Connect to a server first.")) with { Body = Body };
            await SelectAsync(draft);
        }
        finally { navigation.Release(); }
    }
    public async Task DiscardAsync()
    {
        await navigation.WaitAsync();
        try
        {
            if (Editor is not { } editor) return;
            if (!await CanLeaveAsync()) throw LeavingError();
            var observed = Body;
            async Task CheckInputAsync()
            {
                if (!CanLeaveNow()) { await editor.CheckpointAsync(); throw new CompositionInProgressException(); }
                if (Body == observed) return;
                if (!await CanLeaveAsync()) throw LeavingError();
                throw new IOException("Writing changed after discard confirmation. Confirm again to discard the current text.");
            }
            Note? remote = null;
            if (editor.Current.Revision is not null || editor.Current.Pending is not null)
            {
                try { remote = await Connected.ReadAsync(editor.Current.Name); }
                catch (RemoteException error) when (error.Status == 404) { }
            }
            await CheckInputAsync();
            if (remote is not null)
            { var draft = Draft.From(Connected.Origin, remote); await store.WriteAsync(draft); await CheckInputAsync(); Select(draft); }
            else
            { await store.RemoveAsync(editor.Current); await CheckInputAsync(); Editor = null; body = ""; OnPropertyChanged(nameof(Body)); NotifyEditor(); }
            Problem = "";
        }
        finally { navigation.Release(); }
    }
}
