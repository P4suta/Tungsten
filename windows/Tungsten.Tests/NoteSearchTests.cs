using Tungsten.Core;

namespace Tungsten.Tests;

public sealed class NoteSearchTests
{
    [Fact]
    public void MorePagesKeepTheirSubmittedQueryAndRejectOverlappingRequests()
    {
        var search = new NoteSearch();
        var first = search.Begin("alpha")!;
        Assert.Null(first.Cursor); Assert.False(search.CanLoadMore);
        Assert.True(search.Complete(first, "next"));
        var more = search.Begin()!;
        Assert.Equal("alpha", more.Query); Assert.Equal("next", more.Cursor);
        Assert.Null(search.Begin());
        Assert.True(search.Fail(more)); Assert.True(search.CanLoadMore);
    }

    [Theory]
    [InlineData("alpha")]
    [InlineData("beta")]
    public void ObsoleteResponsesCannotReplaceTheCurrentQueryOrPagination(string query)
    {
        var search = new NoteSearch();
        var alpha = search.Begin("alpha")!;
        var current = search.Begin(query)!;
        Assert.False(search.Complete(alpha, "old"));
        Assert.False(search.Fail(alpha)); Assert.False(search.CanLoadMore);
        Assert.True(search.Complete(current, "current"));
        Assert.False(search.Complete(current, "duplicate"));
        var more = search.Begin()!;
        Assert.Equal(query, more.Query); Assert.Equal("current", more.Cursor);
    }
}
