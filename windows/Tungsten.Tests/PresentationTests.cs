using System.Net;
using System.Text.Json;
using Tungsten.Core;

namespace Tungsten.Tests;

public sealed class PresentationTests : IDisposable
{
    private readonly string directory = Path.Combine(Path.GetTempPath(), "tungsten-view-tests-" + Guid.NewGuid().ToString("N"));
    private static readonly Uri Server = new("https://notes.example/");
    private static readonly string Revision = "\"" + new string('a', 64) + "\"";
    private sealed class Handler(Func<HttpRequestMessage, Task<HttpResponseMessage>> action) : HttpMessageHandler
    {
        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken) => action(request);
    }
    private static HttpResponseMessage Response(object value) => new(HttpStatusCode.OK)
    { Content = new StringContent(JsonSerializer.Serialize(value, new JsonSerializerOptions(JsonSerializerDefaults.Web))) };
    private static NoteClient Client(Uri server) => new(server, new Handler(_ => Task.FromResult(Response(new Note("a.md", "remote", Revision)))));

    private sealed class LookupRepository(Draft draft) : IDraftRepository
    {
        public Task<DraftRecovery> LoadAsync(Uri server) => throw new InvalidOperationException("Opening one note must not scan the draft catalog.");
        public Task<Draft?> FindAsync(Uri server, string name) => Task.FromResult<Draft?>(draft.Server == server.AbsoluteUri && draft.Name == name ? draft : null);
        public Task WriteAsync(Draft value, CancellationToken cancellation = default) => Task.CompletedTask;
        public Task RemoveAsync(Draft value) => Task.CompletedTask;
    }

    [Fact]
    public async Task OpeningOneNoteUsesItsDraftWithoutScanningOtherFiles()
    {
        var draft = new Draft(1, Server.AbsoluteUri, "a.md", "local", null, null, null);
        var model = new ClientModel(new LookupRepository(draft), Client);
        await model.ConnectAsync(Server); await model.OpenAsync(draft.Name);
        Assert.Equal("local", model.Body);
    }

    [Fact]
    public async Task DeniedPendingStorageBlocksSavingAndAllowsExplicitRetry()
    {
        var store = new DelayedRepository(new AtomicDraftStore(directory));
        var draft = new Draft(1, Server.AbsoluteUri, "a.md", "local", null, null, null); await store.WriteAsync(draft);
        var model = new ClientModel(store, uri => new(uri, new Handler(request => Task.FromResult(request.Method == HttpMethod.Get
            ? new HttpResponseMessage(HttpStatusCode.NotFound) : Response(new Note(draft.Name, draft.Body, Revision))))));
        await model.ConnectAsync(Server); await model.OpenAsync(draft.Name);
        store.Failure = value => value.Pending is not null ? new UnauthorizedAccessException("Draft access denied.") : null;
        await Assert.ThrowsAsync<UnauthorizedAccessException>(() => model.SaveAsync());
        Assert.Equal(SaveBlock.Uncertain, model.Editor!.Block); Assert.NotEmpty(model.Problem); Assert.Equal("local", model.Body);
        store.Failure = null;
        await model.Editor.RetryAsync(); await model.SaveAsync();
        Assert.True(model.Editor.IsSaved); Assert.Empty(model.Problem);
    }

    [Fact]
    public async Task CreatingAndSwitchingConnectionsPersistsEachServersInput()
    {
        var store = new AtomicDraftStore(directory); var model = new ClientModel(store, Client);
        await model.ConnectAsync(Server); await model.NewAsync(); model.Body = "first server";
        var first = model.Editor!.Current.Name;
        await model.ConnectAsync(new("https://other.example/")); await model.NewAsync(); model.Body = "second server";
        Assert.True(await model.CanLeaveAsync());
        Assert.Equal("first server", Assert.Single((await store.LoadAsync(Server)).Drafts).Body);
        Assert.NotEqual(first, model.Editor!.Current.Name);
    }

    [Fact]
    public async Task DiskFailureBlocksNavigationConnectionChangeAndClosing()
    {
        var store = new AtomicDraftStore(directory); var model = new ClientModel(store, Client);
        await model.ConnectAsync(Server); await model.NewAsync(); model.Body = "keep input";
        Directory.Delete(directory, true); await File.WriteAllTextAsync(directory, "occupied");
        Assert.False(await model.CanLeaveAsync());
        await Assert.ThrowsAsync<IOException>(() => model.ConnectAsync(new("https://other.example/")));
        Assert.Equal(Server, model.Client!.Origin); Assert.Equal("keep input", model.Body);
    }

    [Fact]
    public async Task DelayedOpenPersistsInputTypedWhileLoading()
    {
        var reply = new TaskCompletionSource<HttpResponseMessage>(TaskCreationOptions.RunContinuationsAsynchronously);
        var requested = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        var store = new AtomicDraftStore(directory);
        var model = new ClientModel(store, uri => new(uri, new Handler(_ => { requested.SetResult(); return reply.Task; })));
        await model.ConnectAsync(Server); await model.NewAsync(); model.Body = "before";
        var first = model.Editor!.Current.Name;
        var opening = model.OpenAsync("a.md"); await requested.Task; model.Body = "typed while loading";
        reply.SetResult(Response(new Note("a.md", "remote", Revision))); await opening;
        Assert.Equal("typed while loading", (await store.LoadAsync(Server)).Drafts.Single(d => d.Name == first).Body);
        Assert.Equal("remote", model.Body);
    }

    [Fact]
    public async Task SaveAsNewKeepsTheOriginalDraftAndUsesAnotherIdentity()
    {
        var store = new AtomicDraftStore(directory); var model = new ClientModel(store, Client);
        await model.ConnectAsync(Server); await model.NewAsync(); model.Body = "copy";
        var original = model.Editor!.Current.Name; await model.SaveAsNewAsync();
        Assert.NotEqual(original, model.Editor!.Current.Name); Assert.Equal("copy", model.Body);
        Assert.Equal(2, (await store.LoadAsync(Server)).Drafts.Length);
    }

    [Fact]
    public async Task CommandsFollowConnectionAndSavedNoteAvailability()
    {
        var model = new ClientModel(new AtomicDraftStore(directory), Client);
        Assert.False(model.HasConnection); Assert.False(model.HasSavedNote);
        await model.ConnectAsync(Server);
        Assert.True(model.HasConnection); Assert.False(model.HasSavedNote);
        await model.NewAsync(); Assert.True(model.HasEditor); Assert.False(model.HasSavedNote);
        await model.OpenAsync("a.md"); Assert.True(model.HasSavedNote);
    }

    [Fact]
    public void NativeListItemsSpeakTitlesInsteadOfRecordRepresentations()
    {
        Assert.Equal("Writing", new Summary("a.md", "Writing").ToString());
        Assert.Equal("Earlier writing", new SavedNote(new string('a', 32), "a.md", "Earlier writing", 1, Revision).ToString());
    }

    [Fact]
    public async Task ExplicitDiscardRemovesAnUnsentDraftAndReloadsASavedNote()
    {
        var store = new AtomicDraftStore(directory); var model = new ClientModel(store, Client);
        await model.ConnectAsync(Server); await model.NewAsync(); model.Body = "discard";
        await model.DiscardAsync(); Assert.False(model.HasEditor);
        Assert.Empty((await store.LoadAsync(Server)).Drafts);
        await model.OpenAsync("a.md"); model.Body = "local changes";
        await model.DiscardAsync(); Assert.Equal("remote", model.Body);
        Assert.Equal("remote", Assert.Single((await store.LoadAsync(Server)).Drafts).Body);
    }

    [Fact]
    public async Task AutosaveCheckpointsLaterInputWhileTheNetworkActionIsBusy()
    {
        var requested = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        var reply = new TaskCompletionSource<HttpResponseMessage>(TaskCreationOptions.RunContinuationsAsynchronously);
        var store = new AtomicDraftStore(directory);
        var model = new ClientModel(store, uri => new(uri, new Handler(_ => { requested.SetResult(); return reply.Task; })));
        await model.ConnectAsync(Server); await model.NewAsync(); model.Body = "outgoing";
        var name = model.Editor!.Current.Name; var saving = model.SaveAsync(); await requested.Task;
        model.Body = "typed while sending"; var sent = false;
        try
        {
            await model.AutosaveAsync(false, true, () => { sent = true; return Task.CompletedTask; }).WaitAsync(TimeSpan.FromSeconds(2));
            Assert.False(sent);
            Assert.Equal("typed while sending", Assert.Single((await store.LoadAsync(Server)).Drafts).Body);
        }
        finally { reply.TrySetResult(Response(new Note(name, "outgoing", Revision))); await saving; }
    }

    [Fact]
    public async Task AutosaveDefersCompositionAndStopsSendingAfterLocalStorageFails()
    {
        var store = new AtomicDraftStore(directory); var model = new ClientModel(store, Client);
        await model.ConnectAsync(Server); await model.NewAsync(); model.Body = "composition";
        var sent = false; Task Send() { sent = true; return Task.CompletedTask; }
        await model.AutosaveAsync(true, false, Send);
        Assert.False(sent); Assert.Equal("", Assert.Single((await store.LoadAsync(Server)).Drafts).Body);
        Directory.Delete(directory, true); await File.WriteAllTextAsync(directory, "occupied");
        await model.AutosaveAsync(false, false, Send);
        Assert.False(sent); Assert.Contains("could not be stored", model.Problem);
        Assert.Equal("composition", model.Body);
    }

    [Fact]
    public async Task UndoDuringASaveCheckpointSkipsTheRedundantWrite()
    {
        var store = new DelayedRepository(new AtomicDraftStore(directory));
        var model = new ClientModel(store, Client);
        await model.ConnectAsync(Server); await model.OpenAsync("a.md");
        var acknowledged = model.Body; model.Body = "temporary edit";
        store.Pause = _ => true;
        var saving = model.SaveAsync();
        try { await store.Held.Task.WaitAsync(TimeSpan.FromSeconds(2)); model.Body = acknowledged; }
        finally { store.Released.TrySetResult(); }
        await saving.WaitAsync(TimeSpan.FromSeconds(2));
        Assert.True(model.Editor!.IsSaved); Assert.Equal(SaveBlock.None, model.Editor.Block);
        Assert.Null(model.Editor.Current.Pending);
        var stored = Assert.Single((await store.LoadAsync(Server)).Drafts);
        Assert.Equal(acknowledged, stored.Body); Assert.Null(stored.Pending);
    }

    [Fact]
    public async Task ClosingRejectsANoteSwitchStartedDuringItsCheckpoint()
    {
        var store = new DelayedRepository(new AtomicDraftStore(directory));
        var model = new ClientModel(store, Client);
        await model.ConnectAsync(Server); await model.NewAsync(); model.Body = "first note";
        store.Pause = _ => true;
        var approval = model.CanLeaveAsync();
        await store.Held.Task.WaitAsync(TimeSpan.FromSeconds(2));
        var navigation = model.NewAsync();
        store.Released.TrySetResult();
        await navigation.WaitAsync(TimeSpan.FromSeconds(2)); await approval.WaitAsync(TimeSpan.FromSeconds(2));
        model.Body = "input in the selected note";
        Assert.False(model.CanCloseNow()); Assert.Equal("input in the selected note", model.Body);
        Assert.True(await model.CanLeaveAsync()); Assert.True(model.CanCloseNow());
        Assert.Equal(model.Body, (await store.LoadAsync(Server)).Drafts.Single(d => d.Name == model.Editor!.Current.Name).Body);
    }

    [Theory]
    [InlineData(false)]
    [InlineData(true)]
    public async Task ClosingRechecksInputAndCompositionAfterAnApprovedCheckpoint(bool compose)
    {
        var composing = false;
        var model = new ClientModel(new AtomicDraftStore(directory), Client, () => !composing);
        await model.ConnectAsync(Server); await model.NewAsync();
        Assert.True(await model.CanLeaveAsync()); Assert.True(model.CanCloseNow());
        if (compose) composing = true; else model.Body = "input after the checkpoint";
        Assert.False(model.CanCloseNow());
    }

    [Fact]
    public async Task AutosaveDoesNotSendWhenCompositionStartsDuringItsCheckpoint()
    {
        var store = new DelayedRepository(new AtomicDraftStore(directory));
        var composing = false;
        var model = new ClientModel(store, Client, () => !composing);
        await model.ConnectAsync(Server); await model.NewAsync(); model.Body = "before composition";
        store.Pause = _ => true;
        var sent = false;
        var saving = model.AutosaveAsync(composing, false, () => { sent = true; return Task.CompletedTask; });
        try { await store.Held.Task.WaitAsync(TimeSpan.FromSeconds(2)); composing = true; }
        finally { store.Released.TrySetResult(); }
        await saving;
        Assert.True(composing); Assert.False(sent);
    }

    [Fact]
    public async Task ACheckpointDoesNotAuthorizeLeavingAfterCompositionStarts()
    {
        var store = new DelayedRepository(new AtomicDraftStore(directory));
        var composing = false;
        var model = new ClientModel(store, Client, () => !composing);
        await model.ConnectAsync(Server); await model.NewAsync();
        store.Pause = _ => true;
        var leaving = model.CanLeaveAsync();
        try { await store.Held.Task.WaitAsync(TimeSpan.FromSeconds(2)); composing = true; }
        finally { store.Released.TrySetResult(); }
        Assert.True(composing); Assert.False(await leaving);
        composing = false; Assert.Equal("", model.Problem);
    }

    [Fact]
    public async Task NavigationKeepsTheCurrentEditorWhenCompositionStartsDuringARead()
    {
        var reply = new TaskCompletionSource<HttpResponseMessage>(TaskCreationOptions.RunContinuationsAsynchronously);
        var requested = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        var composing = false;
        var model = new ClientModel(new AtomicDraftStore(directory), uri => new(uri, new Handler(_ => { requested.SetResult(); return reply.Task; })), () => !composing);
        await model.ConnectAsync(Server); await model.NewAsync(); model.Body = "keep the composing note";
        var editor = model.Editor;
        var opening = model.OpenAsync("a.md");
        await requested.Task.WaitAsync(TimeSpan.FromSeconds(2)); composing = true;
        reply.SetResult(Response(new Note("a.md", "remote", Revision)));
        await Assert.ThrowsAsync<CompositionInProgressException>(() => opening);
        Assert.True(composing); Assert.Same(editor, model.Editor); Assert.Equal("keep the composing note", model.Body);
    }

    [Theory]
    [InlineData("open")]
    [InlineData("create")]
    [InlineData("copy")]
    public async Task CompositionDuringTheDestinationWriteKeepsTheCurrentEditor(string operation)
    {
        var composing = false;
        var store = new DelayedRepository(new AtomicDraftStore(directory));
        var model = new ClientModel(store, Client, () => !composing);
        await model.ConnectAsync(Server); await model.NewAsync(); model.Body = "keep the current note";
        var editor = model.Editor; store.Pause = draft => draft.Name != editor!.Current.Name;
        var opening = operation switch { "open" => model.OpenAsync("a.md"), "create" => model.NewAsync(), _ => model.SaveAsNewAsync() };
        try { await store.Held.Task.WaitAsync(TimeSpan.FromSeconds(2)); composing = true; }
        finally { store.Released.TrySetResult(); }
        await Assert.ThrowsAsync<CompositionInProgressException>(() => opening);
        Assert.Same(editor, model.Editor); Assert.Equal("keep the current note", model.Body);
    }

    [Fact]
    public async Task CompositionBeforeTransmissionClearsTheUnsentPendingWrite()
    {
        var composing = false; var sent = false;
        var store = new DelayedRepository(new AtomicDraftStore(directory));
        var model = new ClientModel(store, uri => new(uri, new Handler(_ => { sent = true; throw new InvalidOperationException("No request should be sent during composition."); })), () => !composing);
        await model.ConnectAsync(Server); await model.NewAsync(); model.Body = "keep unsent input";
        store.Pause = draft => draft.Pending is not null;
        var saving = model.SaveAsync();
        try { await store.Held.Task.WaitAsync(TimeSpan.FromSeconds(2)); composing = true; }
        finally { store.Released.TrySetResult(); }
        await saving.WaitAsync(TimeSpan.FromSeconds(2));
        Assert.False(sent); Assert.Null(model.Editor!.Current.Pending); Assert.True(model.Editor.IsDurable);
        Assert.Equal("keep unsent input", model.Body);
    }

    private sealed class DelayedRepository(AtomicDraftStore store) : IDraftRepository
    {
        public Func<Draft, bool>? Pause { get; set; }
        public Func<Draft, Exception?>? Failure { get; set; }
        public TaskCompletionSource Held { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
        public TaskCompletionSource Released { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
        public async Task WriteAsync(Draft draft, CancellationToken cancellation = default)
        {
            if (Failure?.Invoke(draft) is { } error) throw error;
            await store.WriteAsync(draft, cancellation);
            if (Pause?.Invoke(draft) == true) { Pause = null; Held.SetResult(); await Released.Task; }
        }
        public Task<DraftRecovery> LoadAsync(Uri server) => store.LoadAsync(server);
        public Task<Draft?> FindAsync(Uri server, string name) => store.FindAsync(server, name);
        public Task RemoveAsync(Draft draft) => store.RemoveAsync(draft);
    }

    [Theory]
    [InlineData("open")]
    [InlineData("create")]
    [InlineData("copy")]
    public async Task InputDuringTheDestinationsFinalWriteRemainsInThePreviousDraft(string operation)
    {
        var store = new AtomicDraftStore(directory); var repository = new DelayedRepository(store);
        var model = new ClientModel(repository, Client);
        await model.ConnectAsync(Server); await model.NewAsync(); model.Body = "before navigation";
        var original = model.Editor!.Current.Name;
        repository.Pause = draft => draft.Name != original;
        var navigating = operation switch { "open" => model.OpenAsync("a.md"), "create" => model.NewAsync(), _ => model.SaveAsNewAsync() };
        try { await repository.Held.Task.WaitAsync(TimeSpan.FromSeconds(2)); model.Body = "typed during final write"; }
        finally { repository.Released.TrySetResult(); }
        await navigating;
        Assert.Equal("typed during final write", (await store.LoadAsync(Server)).Drafts.Single(d => d.Name == original).Body);
    }

    [Fact]
    public async Task SelectingTheCurrentNoteKeepsItsEditorAndUndoScope()
    {
        var model = new ClientModel(new AtomicDraftStore(directory), Client);
        await model.ConnectAsync(Server); await model.OpenAsync("a.md"); model.Body = "local";
        var editor = model.Editor; await model.OpenAsync("a.md");
        Assert.Same(editor, model.Editor); Assert.Equal("local", model.Body);
    }

    [Fact]
    public async Task DiscardDoesNotReplaceInputTypedAfterConfirmation()
    {
        var reading = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        var reply = new TaskCompletionSource<HttpResponseMessage>(TaskCreationOptions.RunContinuationsAsynchronously);
        var requests = 0; var store = new AtomicDraftStore(directory);
        var model = new ClientModel(store, uri => new(uri, new Handler(_ =>
        {
            if (++requests == 1) return Task.FromResult(Response(new Note("a.md", "remote", Revision)));
            reading.SetResult(); return reply.Task;
        })));
        await model.ConnectAsync(Server); await model.OpenAsync("a.md"); model.Body = "confirmed discard";
        var discarding = model.DiscardAsync(); await reading.Task.WaitAsync(TimeSpan.FromSeconds(2));
        model.Body = "typed after confirmation"; reply.SetResult(Response(new Note("a.md", "remote", Revision)));
        await Assert.ThrowsAsync<IOException>(() => discarding);
        Assert.Equal("typed after confirmation", model.Body);
        Assert.Equal(model.Body, Assert.Single((await store.LoadAsync(Server)).Drafts).Body);
    }

    public void Dispose()
    { if (Directory.Exists(directory)) Directory.Delete(directory, true); else if (File.Exists(directory)) File.Delete(directory); }
}
