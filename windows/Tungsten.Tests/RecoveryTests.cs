using Tungsten.Core;

namespace Tungsten.Tests;

public sealed class RecoveryTests
{
    private static readonly Uri Origin = new("https://notes.example/");
    private static readonly string Revision = "\"" + new string('a', 64) + "\"";
    private static Draft Loaded() => Draft.From(Origin, new("note.md", "before", Revision));

    private sealed class Store : IDraftStore
    {
        public Draft? Durable { get; private set; }
        public bool Fail { get; set; }
        public Task WriteAsync(Draft draft, CancellationToken cancellation = default)
        {
            if (Fail) throw new IOException("Disk full.");
            Durable = draft;
            return Task.CompletedTask;
        }
    }
    private sealed class Service : INoteService
    {
        public Uri Origin => RecoveryTests.Origin;
        public TaskCompletionSource<Note> Reply { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
        public Draft? Sent { get; private set; }
        public Note Remote { get; set; } = new("note.md", "before", Revision);
        public Task<Note> ReadAsync(string name, CancellationToken cancellation = default) => Task.FromResult(Remote);
        public Task<Note> SaveAsync(Draft draft, CancellationToken cancellation = default)
        {
            Sent = draft;
            return Reply.Task;
        }
    }

    [Fact]
    public async Task InputDuringTransmissionSurvivesAcknowledgementAndRestart()
    {
        var service = new Service(); var store = new Store();
        var editor = new EditorSession(Loaded(), service, store);
        editor.Edit("outgoing");
        var write = editor.SaveAsync();
        Assert.Equal("outgoing", store.Durable?.Pending?.Body);
        editor.Edit("later input");
        service.Reply.SetResult(new("note.md", "outgoing", Revision));
        await write;
        Assert.Equal("later input", editor.Current.Body);
        Assert.Equal("outgoing", editor.Current.BaseBody);
        Assert.False(editor.IsSaved);
        Assert.Equal(editor.Current, store.Durable);
        var recovered = new EditorSession(store.Durable!, service, store);
        Assert.Equal("later input", recovered.Current.Body);
    }

    [Theory]
    [InlineData(412, SaveBlock.Conflict)]
    [InlineData(404, SaveBlock.Missing)]
    [InlineData(500, SaveBlock.Uncertain)]
    public async Task FailureStopsAutomaticWritesAndRetainsPendingBody(int status, SaveBlock block)
    {
        var service = new Service(); var store = new Store();
        var editor = new EditorSession(Loaded(), service, store);
        editor.Edit("local");
        var write = editor.SaveAsync();
        service.Reply.SetException(new RemoteException(status));
        await Assert.ThrowsAsync<RemoteException>(() => write);
        Assert.Equal(block, editor.Block);
        Assert.Equal("local", store.Durable?.Pending?.Body);
        var restart = new EditorSession(store.Durable!, service, store);
        Assert.Equal(SaveBlock.Uncertain, restart.Block);
        await restart.SaveAsync();
        Assert.Equal("local", restart.Current.Body);
    }

    [Fact]
    public async Task LocalWriteFailurePreventsTransmissionAndNavigation()
    {
        var service = new Service(); var store = new Store { Fail = true };
        var editor = new EditorSession(Loaded(), service, store);
        editor.Edit("unsent");
        await Assert.ThrowsAsync<IOException>(() => editor.SaveAsync());
        Assert.Null(service.Sent);
        await Assert.ThrowsAsync<IOException>(() => editor.CheckpointAsync());
        Assert.Equal("unsent", editor.Current.Body);
    }

    [Fact]
    public async Task LostAcknowledgementIsReconciledBeforeRetrying()
    {
        var service = new Service { Remote = new("note.md", "sent", Revision) };
        var draft = Loaded() with { Body = "later", Pending = new("sent", Revision) };
        var editor = new EditorSession(draft, service, new Store());
        await editor.RetryAsync();
        Assert.Equal(SaveBlock.None, editor.Block);
        Assert.Equal("later", editor.Current.Body);
        Assert.Equal("sent", editor.Current.BaseBody);
        Assert.Null(editor.Current.Pending);
    }

    [Fact]
    public async Task RefreshProtectsDirtyInputAndConnectionScope()
    {
        var service = new Service { Remote = new("note.md", "remote", "\"" + new string('b', 64) + "\"") };
        var editor = new EditorSession(Loaded(), service, new Store());
        editor.Edit("local"); await editor.RefreshAsync();
        Assert.Equal("local", editor.Current.Body);
        Assert.Equal(SaveBlock.Conflict, editor.Block);
        var restarted = new EditorSession(editor.Current, service, new Store());
        Assert.Equal(SaveBlock.Conflict, restarted.Block);
        Assert.Throws<InvalidDataException>(() => new EditorSession(Loaded() with { Server = "https://other.example/" }, service, new Store()));
    }

    [Fact]
    public async Task InvalidAcknowledgementStopsAutosaveAndRetainsPendingData()
    {
        var service = new Service(); var store = new Store();
        var editor = new EditorSession(Loaded(), service, store);
        editor.Edit("sent"); var sending = editor.SaveAsync();
        service.Reply.SetResult(new("other.md", "wrong", Revision));
        await Assert.ThrowsAsync<InvalidDataException>(() => sending);
        Assert.Equal(SaveBlock.Uncertain, editor.Block);
        Assert.Equal("sent", store.Durable?.Pending?.Body);
    }
    [Fact]
    public async Task CheckpointDuringTransmissionStoresLaterInputWithoutWaitingForNetwork()
    {
        var service = new Service(); var store = new Store(); var editor = new EditorSession(Loaded(), service, store);
        editor.Edit("sent"); var saving = editor.SaveAsync(); editor.Edit("later");
        try
        {
            Assert.True(await editor.CheckpointAsync().WaitAsync(TimeSpan.FromSeconds(1)));
            Assert.Equal("later", store.Durable?.Body); Assert.Equal("sent", store.Durable?.Pending?.Body);
        }
        finally { service.Reply.TrySetResult(new("note.md", "sent", Revision)); await saving; }
    }
    [Fact]
    public async Task OversizedInputStaysDurableWithoutBeingSentAsAnOversizedNote()
    {
        var service = new Service(); var store = new Store(); var editor = new EditorSession(Loaded(), service, store);
        var text = new string('語', Rules.MaxNoteBytes / 3 + 1);
        editor.Edit(text); Assert.True(await editor.CheckpointAsync());
        Assert.Equal(text, store.Durable?.Body);
        await Assert.ThrowsAsync<InvalidDataException>(() => editor.SaveAsync());
        Assert.Null(service.Sent); Assert.Equal(text, editor.Current.Body);
    }

    [Fact]
    public async Task DurabilityIsReportedOnlyAfterTheCurrentDraftWasWritten()
    {
        var store = new Store(); var editor = new EditorSession(Loaded(), new Service(), store);
        editor.Edit("local"); Assert.False(editor.IsDurable);
        Assert.True(await editor.CheckpointAsync()); Assert.True(editor.IsDurable);
        editor.Edit("later"); Assert.False(editor.IsDurable);
        store.Fail = true; await Assert.ThrowsAsync<IOException>(() => editor.CheckpointAsync());
        Assert.False(editor.IsDurable); Assert.Equal("local", store.Durable!.Body);
    }
}
