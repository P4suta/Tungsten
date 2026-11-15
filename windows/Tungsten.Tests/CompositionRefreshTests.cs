using Tungsten.Core;

namespace Tungsten.Tests;

public sealed class CompositionRefreshTests
{
    private sealed class Service : INoteService
    {
        public Uri Origin => new("https://notes.example/");
        public TaskCompletionSource<Note> Reply { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
        public Task<Note> ReadAsync(string name, CancellationToken cancellation = default) => Reply.Task;
        public Task<Note> SaveAsync(Draft draft, CancellationToken cancellation = default) => throw new NotSupportedException();
    }
    private sealed class Store : IDraftStore
    { public Task WriteAsync(Draft draft, CancellationToken cancellation = default) => Task.CompletedTask; }

    [Fact]
    public async Task CompositionStartedDuringAReadPreventsRemoteReplacement()
    {
        var service = new Service();
        var original = Draft.From(service.Origin, new("a.md", "before", "\"" + new string('a', 64) + "\""));
        var editor = new EditorSession(original, service, new Store());
        var composing = false;
        var refresh = editor.RefreshAsync(() => !composing);
        composing = true;
        service.Reply.SetResult(new("a.md", "remote", "\"" + new string('b', 64) + "\""));
        await refresh;
        Assert.Equal(original, editor.Current);
        composing = false; await editor.RefreshAsync(() => !composing);
        Assert.Equal("remote", editor.Current.Body);
    }
}
