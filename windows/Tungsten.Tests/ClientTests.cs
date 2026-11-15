using System.Net;
using System.Text;
using System.Text.Json;
using Tungsten.Core;

namespace Tungsten.Tests;

public sealed class ClientTests
{
    private static readonly Uri Server = new("https://notes.example/");
    private static readonly string Revision = "\"" + new string('a', 64) + "\"";
    private static readonly string Id = new('a', 32);
    private static readonly JsonSerializerOptions Json = new(JsonSerializerDefaults.Web);
    private sealed class Handler(Func<HttpRequestMessage, Task<HttpResponseMessage>> respond) : HttpMessageHandler
    {
        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken) => respond(request);
    }
    private static HttpResponseMessage Response(object body) => new(HttpStatusCode.OK) { Content = new StringContent(JsonSerializer.Serialize(body, Json), Encoding.UTF8, "application/json") };
    private static NoteClient Client(Func<HttpRequestMessage, Task<HttpResponseMessage>> respond) => new(Server, new Handler(respond));

    private sealed class CheckedBody(byte[] body) : MemoryStream(body)
    {
        public override ValueTask<int> ReadAsync(Memory<byte> buffer, CancellationToken cancellation = default)
        {
            Assert.True(cancellation.CanBeCanceled, "Response-body reads need a cancellation deadline.");
            return base.ReadAsync(buffer, cancellation);
        }
    }

    private sealed class WaitingBody : MemoryStream
    {
        public TaskCompletionSource Entered { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
        public override bool CanSeek => false;
        public override async ValueTask<int> ReadAsync(Memory<byte> buffer, CancellationToken cancellation = default)
        { Entered.TrySetResult(); await Task.Delay(Timeout.InfiniteTimeSpan, cancellation); return 0; }
    }
    private sealed class RequestClock : TimeProvider
    {
        public TimeSpan DueTime { get; private set; }
        public DeadlineTimer? Timer { get; private set; }
        public override ITimer CreateTimer(TimerCallback callback, object? state, TimeSpan dueTime, TimeSpan period)
        {
            Assert.Equal(Timeout.InfiniteTimeSpan, period);
            DueTime = dueTime; return Timer = new(callback, state);
        }
        public sealed class DeadlineTimer(TimerCallback callback, object? state) : ITimer
        {
            public bool Disposed { get; private set; }
            public void Expire() { if (!Disposed) callback(state); }
            public bool Change(TimeSpan dueTime, TimeSpan period) => throw new NotSupportedException();
            public void Dispose() => Disposed = true;
            public ValueTask DisposeAsync() { Dispose(); return ValueTask.CompletedTask; }
        }
    }

    [Theory]
    [InlineData(false, false)]
    [InlineData(true, false)]
    [InlineData(false, true)]
    [InlineData(true, true)]
    public async Task AStalledResponseBodyStopsAtItsDeadlineOrCallerCancellation(bool download, bool cancel)
    {
        var body = new WaitingBody(); var clock = new RequestClock();
        using var cancellation = new CancellationTokenSource();
        using var client = new NoteClient(Server, new Handler(_ => Task.FromResult(new HttpResponseMessage(HttpStatusCode.OK) { Content = new StreamContent(body) })), clock);
        var directory = Path.Combine(Path.GetTempPath(), Guid.NewGuid().ToString("N")); Directory.CreateDirectory(directory);
        var path = Path.Combine(directory, "original.txt");
        try
        {
            if (download) await File.WriteAllTextAsync(path, "keep original");
            Task reading = download ? client.DownloadAsync(new(Id + ".txt", "original.txt", 1, Revision), path, cancellation.Token)
                : client.NotesAsync(cancellation: cancellation.Token);
            await body.Entered.Task.WaitAsync(TimeSpan.FromSeconds(2));
            Assert.Equal(TimeSpan.FromSeconds(120), clock.DueTime);
            if (cancel) cancellation.Cancel(); else clock.Timer!.Expire();
            await Assert.ThrowsAnyAsync<OperationCanceledException>(() => reading.WaitAsync(TimeSpan.FromSeconds(2)));
            Assert.True(clock.Timer!.Disposed);
            if (download) { Assert.Equal("keep original", await File.ReadAllTextAsync(path)); Assert.Equal(path, Assert.Single(Directory.GetFiles(directory))); }
            else Assert.Empty(Directory.GetFiles(directory));
        }
        finally { cancellation.Cancel(); Directory.Delete(directory, true); }
    }

    [Theory]
    [InlineData(false)]
    [InlineData(true)]
    public async Task ResponseBodyReadsHaveACancellationDeadline(bool download)
    {
        var bytes = download ? Encoding.UTF8.GetBytes("original") : JsonSerializer.SerializeToUtf8Bytes(new NotePage([], null, 0), Json);
        using var client = Client(_ => Task.FromResult(new HttpResponseMessage(HttpStatusCode.OK) { Content = new StreamContent(new CheckedBody(bytes)) }));
        var path = Path.Combine(Path.GetTempPath(), Guid.NewGuid().ToString("N"));
        try
        {
            if (download) await client.DownloadAsync(new(Id + ".txt", "original.txt", bytes.Length, Revision), path);
            else Assert.Empty((await client.NotesAsync()).Notes);
        }
        finally { if (File.Exists(path)) File.Delete(path); }
    }

    [Fact]
    public async Task ConditionalSavePreservesWireFormatOriginAndUnicodeIdentity()
    {
        var note = new Note("日本語/文章.md", "# Writing", Revision);
        using var client = Client(async request =>
        {
            Assert.Equal(HttpMethod.Put, request.Method);
            Assert.Equal("/api/notes/" + Uri.EscapeDataString(note.Name), request.RequestUri!.AbsolutePath);
            Assert.Equal(Revision, Assert.Single(request.Headers.GetValues("If-Match")));
            Assert.Equal("https://notes.example", Assert.Single(request.Headers.GetValues("Origin")));
            using var body = JsonDocument.Parse(await request.Content!.ReadAsStringAsync());
            Assert.Equal(note.Body, body.RootElement.GetProperty("body").GetString());
            return Response(note);
        });
        Assert.Equal(note, await client.SaveAsync(Draft.From(Server, note)));
    }

    [Fact]
    public async Task CreationOmitsIfMatchAndRejectsAnotherServerBeforeSending()
    {
        var draft = new Draft(1, Server.AbsoluteUri, "created.md", "text", null, null, null);
        using var client = Client(async request =>
        {
            Assert.Equal(HttpMethod.Post, request.Method); Assert.Equal("/api/notes", request.RequestUri!.AbsolutePath);
            Assert.False(request.Headers.Contains("If-Match"));
            using var body = JsonDocument.Parse(await request.Content!.ReadAsStringAsync());
            Assert.Equal(draft.Name, body.RootElement.GetProperty("name").GetString());
            return Response(new Note(draft.Name, draft.Body, Revision));
        });
        Assert.Equal(draft.Body, (await client.SaveAsync(draft)).Body);
        await Assert.ThrowsAsync<InvalidDataException>(() => client.SaveAsync(draft with { Server = "https://other.example/" }));
    }

    [Theory]
    [InlineData(302)]
    [InlineData(404)]
    [InlineData(409)]
    [InlineData(412)]
    [InlineData(413)]
    [InlineData(500)]
    public async Task UnsuccessfulResponsesRetainStatusAndNeverBecomeSuccess(int status)
    {
        using var client = Client(_ => Task.FromResult(new HttpResponseMessage((HttpStatusCode)status)));
        Assert.Equal(status, (await Assert.ThrowsAsync<RemoteException>(() => client.ReadAsync("a.md"))).Status);
    }

    [Theory]
    [InlineData("{\"name\":\"other.md\",\"body\":\"x\",\"revision\":\"bad\"}")]
    [InlineData("{\"name\":\"a.md\"}")]
    [InlineData("null")]
    public async Task MalformedResponsesAreRejected(string json)
    {
        using var client = Client(_ => Task.FromResult(new HttpResponseMessage(HttpStatusCode.OK) { Content = new StringContent(json) }));
        await Assert.ThrowsAsync<InvalidDataException>(() => client.ReadAsync("a.md"));
    }

    [Fact]
    public async Task PagedNotesUseBoundedQueriesAndRejectDuplicateRowsOrRepeatedCursor()
    {
        using var client = Client(request =>
        {
            Assert.Contains("paged=true", request.RequestUri!.Query); Assert.Contains("limit=50", request.RequestUri.Query);
            Assert.Contains("q=hello%20world", request.RequestUri.Query);
            return Task.FromResult(Response(new NotePage([new("a.md", "A")], "continue", 1)));
        });
        Assert.Single((await client.NotesAsync("hello world")).Notes);
        await Assert.ThrowsAsync<InvalidDataException>(() => client.NotesAsync("hello world", "continue"));
        using var duplicates = Client(_ => Task.FromResult(Response(new NotePage([new("a.md", "A"), new("a.md", "A")], null, 2))));
        await Assert.ThrowsAsync<InvalidDataException>(() => duplicates.NotesAsync());
    }

    [Fact]
    public async Task WorkspaceActionsUseExactIdentitiesRevisionsAndMembershipRoutes()
    {
        var workspace = new Workspace("1", "Writing", Revision); var seen = new List<string>();
        using var client = Client(request =>
        {
            seen.Add(request.Method + " " + request.RequestUri!.AbsolutePath);
            if (request.Method == HttpMethod.Delete || request.Method == HttpMethod.Put && request.RequestUri.AbsolutePath == "/api/workspaces/1")
            {
                if (!request.RequestUri.AbsolutePath.Contains("/notes/", StringComparison.Ordinal))
                    Assert.Equal(Revision, Assert.Single(request.Headers.GetValues("If-Match")));
            }
            object result = request.RequestUri.AbsolutePath switch
            {
                "/api/workspaces" when request.Method == HttpMethod.Get => new WorkspacePage([workspace], null),
                "/api/workspaces/1/notes" => new WorkspaceNotes(workspace, [new("a.md", "A")], null),
                _ => workspace
            };
            return Task.FromResult(Response(result));
        });
        Assert.Single((await client.WorkspacesAsync()).Workspaces);
        Assert.Single((await client.WorkspaceNotesAsync("1")).Notes);
        Assert.Equal(workspace, await client.CreateWorkspaceAsync("Writing"));
        await client.RenameWorkspaceAsync(workspace, "Renamed"); await client.MembershipAsync("1", "a.md", true);
        await client.MembershipAsync("1", "a.md", false); await client.DeleteWorkspaceAsync(workspace);
        Assert.Contains("PUT /api/workspaces/1/notes/a.md", seen); Assert.Contains("DELETE /api/workspaces/1", seen);
    }

    [Fact]
    public async Task DetailsHistoryTrashAndRestorationFollowExistingContracts()
    {
        var note = new Note("a.md", "body", Revision); var saved = new SavedNote(Id, note.Name, "Title", 1, Revision);
        using var client = Client(request => Task.FromResult(Response(request.RequestUri!.AbsolutePath switch
        {
            "/api/connections" => new Connections[] { new("a.md", ["b.md"], [Id + ".png"]) },
            "/api/backlinks" => new Summary[] { new("b.md", "Backlink") },
            "/api/history" => new[] { saved },
            "/api/trash" => new TrashPage([saved], null),
            _ => (object)note
        })));
        Assert.Single((await client.ConnectionsAsync("a.md")).Links); Assert.Single(await client.BacklinksAsync("a.md"));
        Assert.Single(await client.HistoryAsync("a.md")); Assert.Equal(note, await client.SavedAsync(saved));
        Assert.Single((await client.TrashAsync()).Entries); Assert.Equal(note, await client.SavedAsync(saved, true));
        Assert.Equal(note, await client.RestoreAsync(saved, note.Name)); await client.EmptyTrashAsync(); await client.DeleteAsync(note);
    }

    [Fact]
    public async Task AttachmentUploadDownloadAndDeletionPreserveOriginalBytes()
    {
        var root = Path.Combine(Path.GetTempPath(), Guid.NewGuid().ToString("N")); Directory.CreateDirectory(root);
        try
        {
            var bytes = Encoding.UTF8.GetBytes("original\0bytes"); var input = Path.Combine(root, "original.txt");
            var output = Path.Combine(root, "download.txt"); await File.WriteAllBytesAsync(input, bytes);
            var attachment = new Attachment(Id + ".txt", "original.txt", bytes.Length, Revision);
            using var client = Client(async request =>
            {
                if (request.Method == HttpMethod.Post)
                { Assert.Equal(bytes, await request.Content!.ReadAsByteArrayAsync()); Assert.True(request.Headers.Contains("X-Attachment-Name")); }
                if (request.RequestUri!.AbsolutePath.StartsWith("/.attachments/", StringComparison.Ordinal))
                    return new(HttpStatusCode.OK) { Content = new ByteArrayContent(bytes) };
                return Response(attachment);
            });
            Assert.Equal(attachment, await client.UploadAsync(input)); Assert.Equal(attachment, await client.AttachmentAsync(attachment.Id));
            await client.DownloadAsync(attachment, output); Assert.Equal(bytes, await File.ReadAllBytesAsync(output));
            await client.DeleteAttachmentAsync(attachment);
        }
        finally { Directory.Delete(root, true); }
    }

    [Fact]
    public async Task SavedDatesUseLocalFormattingAndRejectOutOfRangeServerValues()
    {
        var captured = new SavedNote(Id, "a.md", "Writing", 1700000000000, Revision);
        Assert.True(DateTime.TryParse(captured.CapturedText, System.Globalization.CultureInfo.CurrentCulture, out var date));
        Assert.Equal(2023, date.Year);
        using var client = Client(_ => Task.FromResult(Response(new[] { new { Id, Name = "a.md", Title = "Bad date", Captured = ulong.MaxValue, Revision } })));
        await Assert.ThrowsAsync<InvalidDataException>(() => client.HistoryAsync("a.md"));
    }
}
