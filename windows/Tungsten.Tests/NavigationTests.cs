using Tungsten.Core;

namespace Tungsten.Tests;

public sealed class NavigationTests
{
    [Theory]
    [InlineData(FocusRegion.Editor, false, true, true, FocusRegion.Collections)]
    [InlineData(FocusRegion.Collections, false, true, true, FocusRegion.Notes)]
    [InlineData(FocusRegion.Notes, false, true, true, FocusRegion.Editor)]
    [InlineData(FocusRegion.Collections, true, true, true, FocusRegion.Editor)]
    [InlineData(FocusRegion.Collections, false, false, true, FocusRegion.Editor)]
    [InlineData(FocusRegion.Notes, false, true, false, FocusRegion.Collections)]
    public void RegionMovementCyclesVisiblePanes(FocusRegion current, bool reverse, bool notes, bool editor, FocusRegion expected)
        => Assert.Equal(expected, FocusNavigation.Move(current, reverse, notes, editor));

    [Fact]
    public void SavedTitlesAppearOnceWithoutReorderingLoadedPages()
    {
        Summary[] first = [new("a.md", "First"), new("b.md", "Second")];
        var merged = NoteList.Merge(first, [new("b.md", "Renamed"), new("c.md", "Created")]);
        Assert.Equal(["a.md", "b.md", "c.md"], merged.Select(n => n.Name));
        Assert.Equal("Renamed", merged[1].Title);
        Assert.Equal(merged, NoteList.Merge(merged, [new("c.md", "Created")]));
    }
}
