namespace Tungsten.Core;

public sealed record NoteSearchRequest(string Query, string? Cursor);
public sealed class NoteSearch
{
    private NoteSearchRequest? pending;
    public string Query { get; private set; } = "";
    public string? Cursor { get; private set; }
    public bool CanLoadMore => pending is null && Cursor is not null;
    public NoteSearchRequest? Begin(string? query = null)
    {
        if (query is null && !CanLoadMore) return null;
        if (query is not null) { Query = query; Cursor = null; }
        return pending = new(Query, Cursor);
    }
    public bool Complete(NoteSearchRequest request, string? next)
    {
        if (!ReferenceEquals(request, pending)) return false;
        pending = null; Cursor = next; return true;
    }
    public bool Fail(NoteSearchRequest request)
    {
        if (!ReferenceEquals(request, pending)) return false;
        pending = null; return true;
    }
}
