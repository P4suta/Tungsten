using System.Globalization;
using System.Text;
using System.Text.RegularExpressions;

namespace Tungsten.Core;

public static partial class Rules
{
    public const int MaxNoteBytes = 1024 * 1024;
    public const int MaxAttachmentBytes = 64 * 1024 * 1024;
    public static readonly UTF8Encoding Utf8 = new(false, true);
    public static string NoteName(string value)
    {
        if (value is null) throw new InvalidDataException("Missing note identity.");
        var bytes = Utf8.GetBytes(value); var parts = value.Split('/');
        if (bytes.Length is < 4 or > 240 || !value.EndsWith(".md", StringComparison.Ordinal)
            || value[^4] == ' ' || parts.Length > 16 || value.Any(c => c < 32 || c is '\u007f' or '\\')
            || parts.Any(p => p.Length == 0 || p[0] is '.' or ' ' || p[^1] == ' '))
            throw new InvalidDataException("Invalid note identity.");
        return value;
    }
    public static string Revision(string value) => value is not null && RevisionPattern().IsMatch(value)
        ? value : throw new InvalidDataException("Invalid revision.");
    public static string RecordId(string value) => value is not null && RecordPattern().IsMatch(value)
        ? value : throw new InvalidDataException("Invalid record identity.");
    public static string AttachmentId(string value) => value is not null && AttachmentPattern().IsMatch(value)
        ? value : throw new InvalidDataException("Invalid attachment identity.");
    public static string WorkspaceId(string value) => long.TryParse(value, NumberStyles.None, CultureInfo.InvariantCulture, out var id)
        && id > 0 && id.ToString(CultureInfo.InvariantCulture) == value
        ? value : throw new InvalidDataException("Invalid workspace identity.");
    public static void Body(string body)
    {
        if (body is null || Utf8.GetByteCount(body) > MaxNoteBytes) throw new InvalidDataException("Notes are limited to 1 MiB.");
    }
    public static void LocalBody(string body)
    {
        if (body is null || Utf8.GetByteCount(body) > 8 * MaxNoteBytes) throw new InvalidDataException("The local draft exceeds 8 MiB.");
    }
    public static Uri Origin(string value, bool development = false)
    {
        if (value is null) throw new InvalidDataException("Missing server origin.");
        value = value.Trim();
        var scheme = value.IndexOf("://", StringComparison.Ordinal);
        var suffix = scheme < 0 ? 0 : value.IndexOfAny(['/', '?', '#'], scheme + 3);
        if (value.Any(c => char.IsWhiteSpace(c) || char.IsControl(c) || c == '\\')
            || suffix >= 0 && value[suffix..] != "/" || !Uri.TryCreate(value, UriKind.Absolute, out var uri)
            || !(uri.Scheme == "https" || development && uri.Scheme == "http" && uri.IsLoopback)
            || uri.UserInfo.Length != 0 || uri.AbsolutePath != "/" || uri.Query.Length != 0 || uri.Fragment.Length != 0 || uri.Port == 0)
            throw new InvalidDataException("Enter an HTTPS server origin without a path or credentials.");
        return new Uri(uri.GetLeftPart(UriPartial.Authority) + "/");
    }
    public static string Title(string body) => body.Split('\n').Select(s => s.Trim().TrimStart('#').Trim())
        .FirstOrDefault(s => s.Length > 0) ?? "Untitled Note";
    public static string GeneratedName() => Guid.NewGuid().ToString("N") + ".md";
    [GeneratedRegex("\\A\"[a-fA-F0-9]{64}\"\\z", RegexOptions.CultureInvariant)]
    private static partial Regex RevisionPattern();
    [GeneratedRegex("\\A[a-f0-9]{32}\\z", RegexOptions.CultureInvariant)]
    private static partial Regex RecordPattern();
    [GeneratedRegex("\\A[a-f0-9]{32}\\.[a-zA-Z0-9]{1,12}\\z", RegexOptions.CultureInvariant)]
    private static partial Regex AttachmentPattern();
}

public sealed record Note(string Name, string Body, string Revision)
{
    public Note Validate(string? expected = null)
    {
        Rules.NoteName(Name); Rules.Body(Body); Rules.Revision(Revision);
        if (expected is not null && Name != expected) throw new InvalidDataException("Unexpected note identity.");
        return this;
    }
}
public sealed record Summary(string Name, string Title, string Snippet = "")
{ public override string ToString() => Title; }
public sealed record NotePage(Summary[] Notes, string? Next, int Total);
public sealed record Workspace(string Id, string Title, string Revision);
public sealed record WorkspacePage(Workspace[] Workspaces, string? Next);
public sealed record WorkspaceNotes(Workspace Workspace, Summary[] Notes, string? Next);
public sealed record SavedNote(string Id, string Name, string Title, ulong Captured, string Revision)
{
    public string CapturedText => DateTimeOffset.FromUnixTimeMilliseconds(checked((long)Captured)).ToLocalTime().ToString("g", CultureInfo.CurrentCulture);
    public override string ToString() => Title;
}
public sealed record TrashPage(SavedNote[] Entries, string? Next);
public sealed record Connections(string Name, string[] Links, string[] Attachments);
public sealed record Attachment(string Id, string Name, long Bytes, string Revision);

public sealed class RemoteException(int status) : IOException($"The server returned HTTP {status}.")
{
    public int Status { get; } = status;
}

public sealed record PendingWrite(string Body, string? Revision);
public sealed record Draft(int Format, string Server, string Name, string Body, string? BaseBody, string? Revision, PendingWrite? Pending, SaveBlock Block = SaveBlock.None)
{
    public static Draft New(Uri server) => new(1, server.AbsoluteUri, Rules.GeneratedName(), "", null, null, null);
    public static Draft From(Uri server, Note note) => new(1, server.AbsoluteUri, note.Name, note.Body, note.Body, note.Revision, null);
    public Draft Validate()
    {
        if (Format != 1 || !Enum.IsDefined(Block) || Rules.Origin(Server, true).AbsoluteUri != Server) throw new InvalidDataException("Invalid draft format or server.");
        Rules.NoteName(Name); Rules.LocalBody(Body);
        if ((BaseBody is null) != (Revision is null)) throw new InvalidDataException("Incomplete draft base.");
        if (Revision is not null) Rules.Revision(Revision);
        if (BaseBody is not null) Rules.Body(BaseBody);
        if (Pending is not null) { Rules.Body(Pending.Body); if (Pending.Revision is not null) Rules.Revision(Pending.Revision); }
        return this;
    }
}

public interface INoteService
{
    Uri Origin { get; }
    Task<Note> ReadAsync(string name, CancellationToken cancellation = default);
    Task<Note> SaveAsync(Draft draft, CancellationToken cancellation = default);
}

public interface IDraftStore
{
    Task WriteAsync(Draft draft, CancellationToken cancellation = default);
}

public interface IDraftRepository : IDraftStore
{
    Task<DraftRecovery> LoadAsync(Uri server);
    Task RemoveAsync(Draft draft);
}
