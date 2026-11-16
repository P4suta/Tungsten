using Tungsten.Core;

namespace Tungsten.Tests;

public sealed class LiveMarkdownTests
{
    [Fact]
    public void InactiveMarkdownIsFormattedWithoutChangingItsSource()
    {
        const string source = "# Heading\n\n**strong** and *emphasis* with `code` and [Note](target.md)\n\nEnd";
        var spans = LiveMarkdown.Project(source, source.Length, source.Length);
        Assert.Contains(spans, s => s.Style == MarkdownStyle.Heading && source[s.Start..s.End] == "# Heading");
        Assert.Contains(spans, s => s.Style == MarkdownStyle.Bold && source[s.Start..s.End] == "**strong**");
        Assert.Contains(spans, s => s.Style == MarkdownStyle.Italic && source[s.Start..s.End] == "*emphasis*");
        Assert.Contains(spans, s => s.Style == MarkdownStyle.Code && source[s.Start..s.End] == "`code`");
        Assert.Contains(spans, s => s.Style == MarkdownStyle.Link && source[s.Start..s.End] == "[Note](target.md)");
        Assert.Equal("Heading\n\nstrong and emphasis with code and Note\n\nEnd", Visible(source, spans));
    }

    [Fact]
    public void ActiveBlocksAndSelectionsRevealTheirSyntax()
    {
        const string source = "# Heading\n\n**strong**\n\nEnd";
        Assert.Equal("# Heading\n\nstrong\n\nEnd", Visible(source, LiveMarkdown.Project(source, 3, 3)));
        Assert.Equal(source, Visible(source, LiveMarkdown.Project(source, 0, source.Length)));
        Assert.Equal("Heading\n\n**strong**\n\nEnd", Visible(source, LiveMarkdown.Project(source, 13, 16)));
    }

    [Fact]
    public void EscapesCodeAndUnfinishedSyntaxRemainLiteral()
    {
        const string source = "\\*literal* and **unfinished\n\n```md\n**inside code**\n```\n\nEnd";
        var spans = LiveMarkdown.Project(source, source.Length, source.Length);
        Assert.DoesNotContain(spans, s => s.Style is MarkdownStyle.Bold or MarkdownStyle.Italic);
        Assert.Contains(spans, s => s.Style == MarkdownStyle.Code && source[s.Start..s.End].Contains("**inside code**", StringComparison.Ordinal));
        Assert.Contains("**inside code**", Visible(source, spans));
        Assert.DoesNotContain("```", Visible(source, spans));
    }

    [Fact]
    public void AllProjectionSpansAreBoundedIncludingUnicodeAndNestedMarkup()
    {
        foreach (var source in new[] { "", "\ud83d\udc31 **日本語**", "***nested***", "[**label**](../a.md)", "###", "`", "a\r\nb\rc\n", new string('*', 4096) })
            foreach (var selection in new[] { 0, source.Length / 2, source.Length })
                foreach (var span in LiveMarkdown.Project(source, selection, selection))
                    Assert.True(span.Start >= 0 && span.Start < span.End && span.End <= source.Length);
    }

    [Fact]
    public void NativeParagraphOffsetsPreserveUneditedLineEndingsAndSurrogatePairs()
    {
        var text = new MarkdownSource("# Heading\r\n\ud83d\udc31 text\nEnd\r");
        Assert.Equal("# Heading\r\ud83d\udc31 text\rEnd\r", text.NativeText);
        Assert.Equal(11, text.ToSource(10));
        Assert.Equal(10, text.ToNative(11));
        text.Update("# Heading\r\ud83d\udc31 edited\rEnd\r");
        Assert.Equal("# Heading\r\n\ud83d\udc31 edited\nEnd\r", text.Source);
        text.Update(text.NativeText);
        Assert.Equal("# Heading\r\n\ud83d\udc31 edited\nEnd\r", text.Source);
        text.Update(text.NativeText + "\rnew");
        Assert.EndsWith("End\r\r\nnew", text.Source);
    }

    [Fact]
    public void HeadingsListsQuotesAndStrikethroughRenderTheirParsedMeaning()
    {
        const string source = "## Heading ##\n\n- first\n- second\n\n3. third\n\n> quoted\n\n~~removed~~\n\nEnd";
        var spans = LiveMarkdown.Project(source, source.Length, source.Length);
        Assert.Contains(spans, s => s.Style == MarkdownStyle.Bullet);
        Assert.Contains(spans, s => s.Style == MarkdownStyle.Number && s.Level == 3);
        Assert.Contains(spans, s => s.Style == MarkdownStyle.Quote);
        Assert.Contains(spans, s => s.Style == MarkdownStyle.Strike);
        Assert.Equal("Heading\n\nfirst\nsecond\n\nthird\n\nquoted\n\nremoved\n\nEnd", Visible(source, spans));
    }

    [Fact]
    public void FormattingCommandsInsertMarkdownAndKeepTheSelectedContent()
    {
        var bold = MarkdownFormatting.Wrap("before 日本語 after", 7, 3, MarkdownStyle.Bold);
        Assert.Equal("**日本語**", bold.Text); Assert.Equal(2, bold.SelectionStart); Assert.Equal(3, bold.SelectionLength);
        var code = MarkdownFormatting.Wrap("a`b", 0, 3, MarkdownStyle.Code);
        Assert.Equal("``a`b``", code.Text);
        var empty = MarkdownFormatting.Wrap("", 0, 0, MarkdownStyle.Italic);
        Assert.Equal("**", empty.Text); Assert.Equal(1, empty.SelectionStart);
    }

    [Fact]
    public void EnteringContainerMarkersRevealsTheWholeEditableBlock()
    {
        foreach (var source in new[] { "- **item**\n\nEnd", "> **quote**\n\nEnd" })
            Assert.Equal(source, Visible(source, LiveMarkdown.Project(source, 0, 0)));
    }

    [Fact]
    public void ImagesKeepTheirReadableSourceUntilAnInlineImageRendererExists()
    {
        const string source = "![alt](image.png)\n\nEnd";
        Assert.Equal(source, Visible(source, LiveMarkdown.Project(source, source.Length, source.Length)));
    }

    [Theory]
    [InlineData("```")]
    [InlineData("~~~")]
    [InlineData("````")]
    public void UnfinishedFencesWithoutANewlineHaveBoundedProjection(string source)
    {
        foreach (var selection in new[] { -1, 0, source.Length })
            foreach (var span in LiveMarkdown.Project(source, selection, selection))
                Assert.True(span.Start >= 0 && span.Start < span.End && span.End <= source.Length);
    }

    [Theory]
    [InlineData("````\ncode\n```literal", "code\n```literal")]
    [InlineData("````\ncode\n```", "code\n```")]
    [InlineData("~~~\ncode\n```", "code\n```")]
    [InlineData("~~~\ncode\n~~~literal", "code\n~~~literal")]
    [InlineData("````\ncode\n```literal\n", "code\n```literal\n")]
    [InlineData("````\ncode\n```\n", "code\n```\n")]
    [InlineData("~~~\ncode\n~~~literal\n", "code\n~~~literal\n")]
    public void FenceLikeCodeContentRemainsVisible(string source, string content)
        => Assert.EndsWith(content, Visible(source, LiveMarkdown.Project(source, -1, -1)));

    [Theory]
    [InlineData("  ```\n  code\n  ```")]
    [InlineData("> ```\n> code\n> ```")]
    public void ParsedClosingFencesAreHiddenInsideIndentedAndQuotedBlocks(string source)
    {
        var visible = Visible(source, LiveMarkdown.Project(source, -1, -1));
        Assert.Contains("code", visible); Assert.DoesNotContain("```", visible);
    }

    [Theory]
    [InlineData(" word ", MarkdownStyle.Bold, " **word** ", 3)]
    [InlineData("\tword\r\n", MarkdownStyle.Italic, "\t*word*\r\n", 2)]
    public void EmphasisCommandsKeepBoundaryWhitespaceOutsideDelimiters(string source, MarkdownStyle style, string expected, int selection)
    {
        var insertion = MarkdownFormatting.Wrap(source, 0, source.Length, style);
        Assert.Equal(expected, insertion.Text);
        Assert.Equal(selection, insertion.SelectionStart);
        Assert.Equal("word", insertion.Text.Substring(insertion.SelectionStart, insertion.SelectionLength));
        Assert.Contains(LiveMarkdown.Project("Before " + insertion.Text + " after", -1, -1), span => span.Style == style);
    }

    [Fact]
    public void NativeEditsPreserveParagraphsWhenMixedLineEndingsMeet()
    {
        foreach (var source in new[] { "a\rb\nc", "a\r\nb\nc\r", "\rx\n", "a\nb\rc" })
        {
            var native = MarkdownSource.Normalize(source);
            for (var start = 0; start <= native.Length; start++)
                for (var end = start; end <= native.Length; end++)
                    foreach (var inserted in new[] { "", "x", "\r", "\r\r" })
                    {
                        var expected = native[..start] + inserted + native[end..];
                        var text = new MarkdownSource(source);
                        text.Update(expected);
                        Assert.Equal(expected, text.NativeText);
                        Assert.Equal(expected, MarkdownSource.Normalize(text.Source));
                    }
        }
    }

    [Fact]
    public void ParserDepthLimitsLeaveTheOriginalSourceEditable()
    {
        var source = new string('>', 256) + " text";
        var spans = LiveMarkdown.Project(source, -1, -1);
        Assert.Empty(spans); Assert.Equal(source, Visible(source, spans));
    }

    [Theory]
    [InlineData(" a ", " a ")]
    [InlineData("  a  ", "  a  ")]
    [InlineData(" ", " ")]
    [InlineData("  ", "  ")]
    [InlineData("`a", "`a")]
    [InlineData("a`", "a`")]
    [InlineData(" a", " a")]
    [InlineData(" \n ", "  ")]
    [InlineData("\r\na\n", " a")]
    public void CodeCommandsPreserveRenderedBoundarySpaces(string source, string content)
    {
        var insertion = MarkdownFormatting.Wrap(source, 0, source.Length, MarkdownStyle.Code);
        Assert.Equal("<p><code>" + content + "</code></p>\n", Markdig.Markdown.ToHtml(insertion.Text));
        Assert.Equal(source, insertion.Text.Substring(insertion.SelectionStart, insertion.SelectionLength));
    }

    [Theory]
    [InlineData('a')]
    [InlineData('語')]
    public void NativeInputRejectsTheLocalByteLimitBeforeChangingAcceptedSource(char unit)
    {
        var limit = 8 * Rules.MaxNoteBytes; var size = Rules.Utf8.GetByteCount(unit.ToString());
        var source = new string(unit, limit / size) + new string('a', limit % size);
        var text = new MarkdownSource(source); var native = text.NativeText;
        Assert.Throws<InvalidDataException>(() => text.Update(native + unit));
        Assert.Throws<InvalidDataException>(() => text.WithReplacement(new(0, 0), unit.ToString()));
        Assert.Equal(source, text.Source); Assert.Equal(native, text.NativeText);
    }

    [Theory]
    [InlineData("a\r\nb", 0, 4, "a\nc", "a\nc")]
    [InlineData("a\nb", 0, 3, "a\r\nc", "a\r\nc")]
    [InlineData("a\r\nb\r\ntail", 3, 1, "first\nsecond", "a\r\nfirst\nsecond\r\ntail")]
    public void ReplacementPreservesIncomingAndSurroundingSourceLineEndings(string source, int start, int length, string inserted, string expected)
    {
        var text = new MarkdownSource(source);
        var changed = text.WithReplacement(new(start, length), inserted);
        Assert.Equal(expected, changed.Source); Assert.Equal(MarkdownSource.Normalize(expected), changed.NativeText);
        Assert.Equal(source, text.Source);
        var history = new MarkdownHistory(source); history.Select(new(start, length));
        history.Record(changed.Source, new(start + inserted.Length, 0), false);
        Assert.Equal(source, history.Undo().Source); Assert.Equal(expected, history.Redo().Source);
    }

    private static string Visible(string source, IEnumerable<MarkdownSpan> spans)
    {
        var hidden = new bool[source.Length];
        foreach (var span in spans.Where(s => s.Style == MarkdownStyle.Hidden)) Array.Fill(hidden, true, span.Start, span.End - span.Start);
        return string.Concat(source.Where((_, index) => !hidden[index]));
    }
}
