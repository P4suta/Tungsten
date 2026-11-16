using System.Net.Http.Headers;
using System.Text.Json;

namespace Tungsten.Core;

public sealed class NoteClient(Uri origin, HttpMessageHandler? handler = null, TimeProvider? timeProvider = null) : INoteService, IDisposable
{
    public Uri Origin { get; } = Rules.Origin(origin.AbsoluteUri, true);
    private readonly HttpClient http = new(handler ?? new HttpClientHandler { AllowAutoRedirect = false }) { Timeout = TimeSpan.FromSeconds(120) };
    private static readonly JsonSerializerOptions Json = new(JsonSerializerDefaults.Web) { RespectRequiredConstructorParameters = true };

    private HttpRequestMessage Request(HttpMethod method, string[] segments, object? body = null, string? revision = null, string? query = null)
    {
        var path = string.Join('/', segments.Select(Uri.EscapeDataString));
        var request = new HttpRequestMessage(method, new Uri(Origin, path + (query is null ? "" : "?" + query)));
        request.Headers.Add("Origin", Origin.GetLeftPart(UriPartial.Authority));
        request.Headers.Accept.Add(new("application/json"));
        if (revision is not null) request.Headers.TryAddWithoutValidation("If-Match", Rules.Revision(revision));
        if (body is not null) request.Content = new ByteArrayContent(JsonSerializer.SerializeToUtf8Bytes(body, Json));
        else if (method == HttpMethod.Delete) request.Content = new ByteArrayContent([]);
        if (request.Content is not null) request.Content.Headers.ContentType = new("application/json");
        return request;
    }
    private async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellation = default)
    {
        var response = await http.SendAsync(request, HttpCompletionOption.ResponseHeadersRead, cancellation);
        if (response.IsSuccessStatusCode) return response;
        var status = (int)response.StatusCode; response.Dispose(); throw new RemoteException(status);
    }
    private static async Task CopyBoundedAsync(HttpResponseMessage response, Stream output, long limit, CancellationToken cancellation = default)
    {
        if (response.Content.Headers.ContentLength is long length && length > limit) throw new InvalidDataException("Response exceeds the size limit.");
        await using var stream = await response.Content.ReadAsStreamAsync(cancellation);
        var buffer = new byte[65536]; long count = 0;
        while (true)
        {
            var size = await stream.ReadAsync(buffer, cancellation); if (size == 0) break;
            count += size; if (count > limit) throw new InvalidDataException("Response exceeds the size limit.");
            await output.WriteAsync(buffer.AsMemory(0, size), cancellation);
        }
    }
    private async Task<T> JsonAsync<T>(HttpRequestMessage request, CancellationToken cancellation = default)
    {
        using var deadline = new CancellationTokenSource(http.Timeout, timeProvider ?? TimeProvider.System);
        using var scope = CancellationTokenSource.CreateLinkedTokenSource(cancellation, deadline.Token);
        using (request)
        using (var response = await SendAsync(request, scope.Token))
        {
            using var bytes = new MemoryStream(); await CopyBoundedAsync(response, bytes, 8 * Rules.MaxNoteBytes, scope.Token);
            try { return JsonSerializer.Deserialize<T>(bytes.ToArray(), Json) ?? throw new InvalidDataException("Missing response."); }
            catch (JsonException error) { throw new InvalidDataException("Invalid server response.", error); }
        }
    }
    private async Task EmptyAsync(HttpRequestMessage request)
    { using (request) using (await SendAsync(request)) { } }
    private static string Query(params (string Key, string? Value)[] fields) => string.Join('&', fields.Where(p => p.Value is not null)
        .Select(p => Uri.EscapeDataString(p.Key) + "=" + Uri.EscapeDataString(p.Value!)));
    private static void Page<T>(T[] rows, Func<T, string> identity, int limit, string? cursor, string? previous)
    {
        if (rows is null || rows.Length > limit || rows.Any(r => r is null)
            || rows.Select(identity).Distinct(StringComparer.Ordinal).Count() != rows.Length
            || cursor is not null && (cursor.Length is 0 or > 4096 || cursor == previous))
            throw new InvalidDataException("Invalid server page.");
    }
    private static string SummaryIdentity(Summary item)
    { if (item.Title is null || item.Snippet is null) throw new InvalidDataException("Missing note summary."); return Rules.NoteName(item.Name); }
    private static Workspace CheckWorkspace(Workspace value, string? expected = null)
    {
        if (value is null || value.Title is null || expected is not null && expected != value.Id) throw new InvalidDataException("Unexpected workspace.");
        Rules.WorkspaceId(value.Id); Rules.Revision(value.Revision); return value;
    }
    private static string SavedIdentity(SavedNote item)
    { Rules.NoteName(item.Name); Rules.Revision(item.Revision); if (item.Title is null || item.Captured > 253402300799999) throw new InvalidDataException("Invalid saved note metadata."); return Rules.RecordId(item.Id); }
    private static Attachment CheckAttachment(Attachment value, string? expected = null)
    {
        if (value is null || value.Name is null || value.Bytes is < 0 or > Rules.MaxAttachmentBytes || expected is not null && value.Id != expected)
            throw new InvalidDataException("Invalid attachment metadata.");
        Rules.AttachmentId(value.Id); Rules.Revision(value.Revision); return value;
    }
    public async Task<Note> ReadAsync(string name, CancellationToken cancellation = default) =>
        (await JsonAsync<Note>(Request(HttpMethod.Get, ["api", "notes", Rules.NoteName(name)]), cancellation)).Validate(name);
    public async Task<Note> SaveAsync(Draft draft, CancellationToken cancellation = default)
    {
        draft.Validate(); Rules.Body(draft.Body);
        if (draft.Server != Origin.AbsoluteUri) throw new InvalidDataException("Draft belongs to another server.");
        var request = draft.Revision is null
            ? Request(HttpMethod.Post, ["api", "notes"], new { draft.Name, draft.Body })
            : Request(HttpMethod.Put, ["api", "notes", draft.Name], new { draft.Body }, draft.Revision);
        var note = (await JsonAsync<Note>(request, cancellation)).Validate(draft.Name);
        if (note.Body != draft.Body) throw new InvalidDataException("Unexpected saved text.");
        return note;
    }
    public async Task<NotePage> NotesAsync(string query = "", string? cursor = null, CancellationToken cancellation = default)
    {
        var page = await JsonAsync<NotePage>(Request(HttpMethod.Get, ["api", "notes"], query: Query(("paged", "true"), ("limit", "50"), ("q", query), ("cursor", cursor))), cancellation);
        Page(page.Notes, SummaryIdentity, 50, page.Next, cursor);
        if (page.Total < page.Notes.Length) throw new InvalidDataException("Invalid total."); return page;
    }
    public Task DeleteAsync(Note note) => EmptyAsync(Request(HttpMethod.Delete, ["api", "notes", note.Validate().Name], revision: note.Revision));
    public async Task<WorkspacePage> WorkspacesAsync(string? after = null)
    {
        var page = await JsonAsync<WorkspacePage>(Request(HttpMethod.Get, ["api", "workspaces"], query: Query(("after", after))));
        Page(page.Workspaces, w => CheckWorkspace(w).Id, 50, page.Next, after);
        if (page.Next is not null && (page.Workspaces.Length == 0 || page.Workspaces[^1].Id != page.Next
            || long.Parse(Rules.WorkspaceId(page.Next), System.Globalization.CultureInfo.InvariantCulture) <= long.Parse(after ?? "0", System.Globalization.CultureInfo.InvariantCulture)))
            throw new InvalidDataException("Invalid workspace cursor.");
        return page;
    }
    public async Task<WorkspaceNotes> WorkspaceNotesAsync(string id, string? after = null)
    {
        var page = await JsonAsync<WorkspaceNotes>(Request(HttpMethod.Get, ["api", "workspaces", Rules.WorkspaceId(id), "notes"], query: Query(("after", after))));
        CheckWorkspace(page.Workspace, id); Page(page.Notes, SummaryIdentity, 50, page.Next, after); return page;
    }
    public async Task<Workspace> CreateWorkspaceAsync(string title) => CheckWorkspace(await JsonAsync<Workspace>(Request(HttpMethod.Post, ["api", "workspaces"], new { title })));
    public async Task<Workspace> RenameWorkspaceAsync(Workspace workspace, string title) => CheckWorkspace(await JsonAsync<Workspace>(Request(HttpMethod.Put,
        ["api", "workspaces", CheckWorkspace(workspace).Id], new { title }, workspace.Revision)), workspace.Id);
    public Task DeleteWorkspaceAsync(Workspace workspace) => EmptyAsync(Request(HttpMethod.Delete, ["api", "workspaces", CheckWorkspace(workspace).Id], revision: workspace.Revision));
    public async Task<Workspace> MembershipAsync(string workspace, string note, bool included) => CheckWorkspace(await JsonAsync<Workspace>(Request(
        included ? HttpMethod.Put : HttpMethod.Delete, ["api", "workspaces", Rules.WorkspaceId(workspace), "notes", Rules.NoteName(note)], included ? new { } : null)), workspace);
    public async Task<Connections> ConnectionsAsync(string name)
    {
        var values = await JsonAsync<Connections[]>(Request(HttpMethod.Get, ["api", "connections"], query: Query(("name", Rules.NoteName(name)), ("limit", "120"))));
        Page(values, c => Rules.NoteName(c.Name), 120, null, null);
        foreach (var value in values)
        {
            if (value.Links is null || value.Attachments is null) throw new InvalidDataException("Missing connections.");
            foreach (var link in value.Links) Rules.NoteName(link);
            foreach (var id in value.Attachments) Rules.AttachmentId(id);
        }
        return values.SingleOrDefault(v => v.Name == name) ?? throw new InvalidDataException("Missing connections.");
    }
    public async Task<Summary[]> BacklinksAsync(string name)
    {
        var rows = await JsonAsync<Summary[]>(Request(HttpMethod.Get, ["api", "backlinks"], query: Query(("name", Rules.NoteName(name)))));
        Page(rows, SummaryIdentity, 120, null, null); return rows;
    }
    public async Task<SavedNote[]> HistoryAsync(string name)
    {
        var rows = await JsonAsync<SavedNote[]>(Request(HttpMethod.Get, ["api", "history"], query: Query(("name", Rules.NoteName(name)))));
        Page(rows, SavedIdentity, 50, null, null);
        if (rows.Any(r => r.Name != name)) throw new InvalidDataException("Unexpected history identity."); return rows;
    }
    public async Task<Note> SavedAsync(SavedNote record, bool trash = false) => (await JsonAsync<Note>(Request(HttpMethod.Get,
        ["api", trash ? "trash" : "history", SavedIdentity(record)]))).Validate(record.Name);
    public async Task<TrashPage> TrashAsync(string? cursor = null)
    {
        var page = await JsonAsync<TrashPage>(Request(HttpMethod.Get, ["api", "trash"], query: Query(("page", "true"), ("cursor", cursor))));
        Page(page.Entries, SavedIdentity, 50, page.Next, cursor); return page;
    }
    public async Task<Note> RestoreAsync(SavedNote record, string name) => (await JsonAsync<Note>(Request(HttpMethod.Post,
        ["api", "trash", SavedIdentity(record), "restore"], new { name = Rules.NoteName(name) }))).Validate(name);
    public Task EmptyTrashAsync() => EmptyAsync(Request(HttpMethod.Delete, ["api", "trash"]));
    public async Task<Attachment> AttachmentAsync(string id) => CheckAttachment(await JsonAsync<Attachment>(Request(HttpMethod.Get, ["api", "attachments", Rules.AttachmentId(id)])), id);
    public async Task<Attachment> UploadAsync(string path)
    {
        var stream = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read, 65536, FileOptions.Asynchronous);
        await using (stream)
        {
            if (stream.Length > Rules.MaxAttachmentBytes) throw new InvalidDataException("Attachments are limited to 64 MiB.");
            var name = Uri.EscapeDataString(Path.GetFileName(path)); if (name.Length > 720) throw new InvalidDataException("Attachment name is too long.");
            var request = Request(HttpMethod.Post, ["api", "attachments"]); request.Headers.Add("X-Attachment-Name", name);
            request.Content = new StreamContent(stream); request.Content.Headers.ContentType = new MediaTypeHeaderValue("application/octet-stream");
            request.Content.Headers.ContentLength = stream.Length;
            return CheckAttachment(await JsonAsync<Attachment>(request));
        }
    }
    public async Task DownloadAsync(Attachment attachment, string path, CancellationToken cancellation = default)
    {
        CheckAttachment(attachment);
        using var deadline = new CancellationTokenSource(http.Timeout, timeProvider ?? TimeProvider.System);
        using var scope = CancellationTokenSource.CreateLinkedTokenSource(cancellation, deadline.Token);
        using var request = Request(HttpMethod.Get, [".attachments", attachment.Id]);
        using var response = await SendAsync(request, scope.Token);
        var staging = path + "." + Guid.NewGuid().ToString("N") + ".tmp";
        Directory.CreateDirectory(staging);
        var temporary = Path.Combine(staging, Path.GetFileName(path));
        try
        {
            await using (var file = new FileStream(temporary, FileMode.CreateNew, FileAccess.Write, FileShare.None, 65536, FileOptions.Asynchronous))
            {
                await CopyBoundedAsync(response, file, Rules.MaxAttachmentBytes, scope.Token);
                if (file.Length != attachment.Bytes) throw new InvalidDataException("Incomplete original attachment.");
                await file.FlushAsync(scope.Token); file.Flush(true);
            }
            if (OperatingSystem.IsWindows()) await AttachmentSecurity.CheckAsync(temporary, request.RequestUri!, scope.Token);
            scope.Token.ThrowIfCancellationRequested();
            File.Move(temporary, path, true);
        }
        finally { Directory.Delete(staging, true); }
    }
    public Task DeleteAttachmentAsync(Attachment attachment) => EmptyAsync(Request(HttpMethod.Delete, ["api", "attachments", CheckAttachment(attachment).Id], revision: attachment.Revision));
    public void Dispose() => http.Dispose();
}
