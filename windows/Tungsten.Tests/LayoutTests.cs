using Tungsten.Core;

namespace Tungsten.Tests;

public sealed class LayoutTests
{
    [Theory]
    [InlineData(320, false, true, true, false, false)]
    [InlineData(640, true, true, false, true, true)]
    [InlineData(641, false, false, true, true, false)]
    [InlineData(1280, true, false, true, true, false)]
    public void AvailableContentWidthChoosesTheWindowsListDetailPattern(double width, bool selected, bool stacked, bool notes, bool editor, bool back)
        => Assert.Equal(new ListDetailLayout(stacked, notes, editor, back), ListDetailLayout.For(width, selected));
}
