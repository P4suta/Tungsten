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

    private static string Visible(string source, IEnumerable<MarkdownSpan> spans)
    {
        var hidden = new bool[source.Length];
        foreach (var span in spans.Where(s => s.Style == MarkdownStyle.Hidden)) Array.Fill(hidden, true, span.Start, span.End - span.Start);
        return string.Concat(source.Where((_, index) => !hidden[index]));
    }
}
