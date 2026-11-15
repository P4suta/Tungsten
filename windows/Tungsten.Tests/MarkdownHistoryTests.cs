using Tungsten.Core;

namespace Tungsten.Tests;

public sealed class MarkdownHistoryTests
{
    [Fact]
    public void TypingIsOneUndoGroupAndRedoRestoresTextAndSelection()
    {
        var history = new MarkdownHistory("# Title\r\n\r\n");
        history.Select(new(11, 0));
        history.Record("# Title\r\n\r\na", new(12, 0));
        history.Record("# Title\r\n\r\nab", new(13, 0));
        Assert.True(history.CanUndo); Assert.False(history.CanRedo);
        Assert.Equal(new MarkdownSnapshot("# Title\r\n\r\n", new(11, 0)), history.Undo());
        Assert.False(history.CanUndo); Assert.True(history.CanRedo);
        Assert.Equal(new MarkdownSnapshot("# Title\r\n\r\nab", new(13, 0)), history.Redo());
    }

    [Fact]
    public void SourceReplacementPreservesMixedLineEndingsAndUndoBranches()
    {
        var history = new MarkdownHistory("a\r\nb\nc\r");
        history.Select(new(3, 1)); history.Record("a\r\n**b**\nc\r", new(5, 1), false);
        Assert.Equal("a\r\nb\nc\r", history.Undo().Source);
        Assert.Equal(new TextSelection(3, 1), history.Selection);
        history.Record("a\r\nx\nc\r", new(4, 0));
        Assert.False(history.CanRedo); Assert.Equal("a\r\nb\nc\r", history.Undo().Source);
    }

    [Fact]
    public void MovingTheCaretAndAtomicCommandsSeparateUndoGroups()
    {
        var history = new MarkdownHistory("");
        history.Record("a", new(1, 0)); history.Select(new(0, 0)); history.Record("ba", new(1, 0));
        Assert.Equal("a", history.Undo().Source);
        Assert.Equal("", history.Undo().Source);
        history.Record("**bold**", new(2, 4), false);
        history.Record("**bold**x", new(9, 0));
        Assert.Equal("**bold**", history.Undo().Source);
    }

    [Fact]
    public void ImeCompositionCommitsOneChangeAndCancellationAddsNoHistory()
    {
        var history = new MarkdownHistory("before"); history.Select(new(6, 0));
        history.BeginComposition(); history.Record("beforeに", new(7, 0)); history.Record("before日本語", new(9, 0)); history.EndComposition();
        Assert.Equal("before", history.Undo().Source); Assert.Equal("before日本語", history.Redo().Source);
        history.BeginComposition(); history.Record("before日本語a", new(10, 0)); history.Record("before日本語", new(9, 0)); history.EndComposition();
        Assert.Equal("before", history.Undo().Source);
    }

    [Fact]
    public void LongEditSequencesKeepBoundedHistoryAndValidSourceOffsets()
    {
        var history = new MarkdownHistory("");
        for (var index = 0; index < 300; index++) history.Record(index.ToString(System.Globalization.CultureInfo.InvariantCulture), new(0, 0), false);
        var count = 0;
        while (history.CanUndo) { var state = history.Undo(); Assert.InRange(state.Selection.Start + state.Selection.Length, 0, state.Source.Length); count++; }
        Assert.Equal(256, count);
        Assert.Throws<ArgumentOutOfRangeException>(() => history.Select(new(-1, 0)));
        Assert.Throws<ArgumentOutOfRangeException>(() => history.Select(new(history.Source.Length + 1, 0)));
    }

    [Fact]
    public void TheSelectionAppliedByAFormattingCommandBelongsToItsRedoRecord()
    {
        var history = new MarkdownHistory("word"); history.Select(new(0, 4));
        history.Record("**word**", new(8, 0), false); history.AmendSelection(new(2, 4));
        Assert.Equal(new TextSelection(0, 4), history.Undo().Selection);
        Assert.Equal(new TextSelection(2, 4), history.Redo().Selection);
    }
}
