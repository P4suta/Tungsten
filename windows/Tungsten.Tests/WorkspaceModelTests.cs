using System.Net;
using System.Text.Json;
using Tungsten.Core;

namespace Tungsten.Tests;

public sealed class WorkspaceModelTests : IDisposable
{
    private readonly string directory = Path.Combine(Path.GetTempPath(), "tungsten-workspaces-" + Guid.NewGuid().ToString("N"));
    private static readonly Uri Server = new("https://notes.example/");
    private static string Revision(char digit) => "\"" + new string(digit, 64) + "\"";
    private sealed class Handler(Queue<HttpResponseMessage> responses, List<(string Method, string? Revision)> requests) : HttpMessageHandler
    {
        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellation)
        { requests.Add((request.Method.Method, request.Headers.IfMatch.SingleOrDefault()?.ToString())); return Task.FromResult(responses.Dequeue()); }
    }
    private static HttpResponseMessage Response(object value) => new(HttpStatusCode.OK)
    { Content = new StringContent(JsonSerializer.Serialize(value, new JsonSerializerOptions(JsonSerializerDefaults.Web))) };

    [Fact]
    public async Task WorkspaceChangesPreserveTheEditorAndUseTheLatestMembershipRevision()
    {
        var original = new Workspace("1", "Writing", Revision('a'));
        var membership = original with { Revision = Revision('b') };
        var renamed = membership with { Title = "Research", Revision = Revision('c') };
        Queue<HttpResponseMessage> replies = new([Response(new WorkspacePage([original], null)), Response(membership), Response(renamed), new(HttpStatusCode.NoContent), Response(new Workspace("2", "Personal", Revision('d')))]);
        List<(string Method, string? Revision)> requests = [];
        var model = new ClientModel(new AtomicDraftStore(directory), uri => new(uri, new Handler(replies, requests)));
        await model.ConnectAsync(Server); await model.NewAsync(); model.Body = "keep writing";
        var editor = model.Editor;
        await model.RefreshWorkspacesAsync(); Assert.Equal(original, Assert.Single(model.Workspaces));
        await model.MembershipAsync("1", editor!.Current.Name, true);
        await model.RenameWorkspaceAsync("1", "Research");
        Assert.Equal(Revision('b'), requests[2].Revision);
        await model.DeleteWorkspaceAsync("1"); Assert.Equal(Revision('c'), requests[3].Revision);
        await model.CreateWorkspaceAsync("Personal");
        Assert.Equal("2", Assert.Single(model.Workspaces).Id);
        Assert.Same(editor, model.Editor); Assert.Equal("keep writing", model.Body);
        Assert.True(await model.CanLeaveAsync());
    }

    [Fact]
    public async Task RejectedWorkspaceWritesPreserveTheObservedWorkspace()
    {
        var workspace = new Workspace("1", "Writing", Revision('a'));
        Queue<HttpResponseMessage> replies = new([Response(new WorkspacePage([workspace], null)), new(HttpStatusCode.PreconditionFailed)]);
        var model = new ClientModel(new AtomicDraftStore(directory), uri => new(uri, new Handler(replies, [])));
        await model.ConnectAsync(Server); await model.RefreshWorkspacesAsync();
        await Assert.ThrowsAsync<RemoteException>(() => model.RenameWorkspaceAsync("1", "Rejected"));
        Assert.Equal(workspace, Assert.Single(model.Workspaces));
    }

    public void Dispose() { if (Directory.Exists(directory)) Directory.Delete(directory, true); }
}
