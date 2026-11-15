using Markdig;
using Markdig.Syntax;
using Markdig.Syntax.Inlines;
using Markdig.Extensions.EmphasisExtras;
using System.Globalization;
using System.Text.RegularExpressions;

namespace Tungsten.Core;

public enum MarkdownStyle { Heading, Bold, Italic, Code, Link, Hidden, Bullet, Number, Quote, Strike }
public sealed record MarkdownSpan(int Start, int End, MarkdownStyle Style, int Level = 0);
public static class LiveMarkdown
{
    private static readonly MarkdownPipeline Pipeline = new MarkdownPipelineBuilder().UsePreciseSourceLocation().UseEmphasisExtras(EmphasisExtraOptions.Strikethrough).Build();
    private static readonly Regex ListMarker = new(@"\A {0,3}(?:[-+*]|(?<number>[0-9]{1,9})[.)])[ \t]+", RegexOptions.CultureInvariant | RegexOptions.NonBacktracking);
    internal static MarkdownDocument Parse(string source) => Markdown.Parse(source, Pipeline);
    public static MarkdownSpan[] Project(string source, int selectionStart, int selectionEnd)
    {
        var document = Parse(source);
        var result = new List<MarkdownSpan>();
        void Add(int start, int end, MarkdownStyle style, int level = 0)
        { if (start >= 0 && start < end && end <= source.Length) result.Add(new(start, end, style, level)); }
        bool Active(int start, int end) => selectionStart == selectionEnd ? selectionStart >= start && selectionStart <= end
            : selectionStart < end && selectionEnd > start;
        int LineEnd(int start) { while (start < source.Length && source[start] is not ('\r' or '\n')) start++; return start; }
        foreach (var item in document.Descendants().OfType<ListItemBlock>())
        {
            var start = item.Span.Start; var end = LineEnd(start);
            if (Active(start, item.Span.End + 1)) continue;
            var marker = ListMarker.Match(source[start..end]);
            if (!marker.Success) continue;
            var number = marker.Groups["number"];
            Add(start, end, number.Success ? MarkdownStyle.Number : MarkdownStyle.Bullet, number.Success ? int.Parse(number.Value, CultureInfo.InvariantCulture) : 0);
            Add(start, start + marker.Length, MarkdownStyle.Hidden);
        }
        foreach (var quote in document.Descendants().OfType<QuoteBlock>())
        {
            var start = quote.Span.Start; var end = quote.Span.End + 1;
            Add(start, end, MarkdownStyle.Quote);
            if (Active(start, end)) continue;
            for (var line = start; line < end;)
            {
                var marker = line;
                while (marker < end && source[marker] == ' ') marker++;
                if (marker < end && source[marker] == '>') Add(marker, marker + (marker + 1 < end && source[marker + 1] == ' ' ? 2 : 1), MarkdownStyle.Hidden);
                line = LineEnd(line); while (line < end && source[line] is '\r' or '\n') line++;
            }
        }
        foreach (var block in document.Descendants().OfType<LeafBlock>())
        {
            var start = block.Span.Start; var end = block.Span.End + 1;
            if (block is HeadingBlock { IsSetext: false }) end = LineEnd(start);
            var lineStart = start == 0 ? 0 : source.LastIndexOfAny(['\r', '\n'], start - 1) + 1;
            var active = Active(lineStart, end);
            void Hide(int from, int to) { if (!active) Add(from, to, MarkdownStyle.Hidden); }
            if (block is HeadingBlock heading)
            {
                if (!heading.IsSetext) end = LineEnd(start);
                Add(start, end, MarkdownStyle.Heading, heading.Level);
                if (!heading.IsSetext)
                {
                    var content = start;
                    while (content < end && source[content] == '#') content++;
                    while (content < end && source[content] is ' ' or '\t') content++;
                    Hide(start, content);
                    if (heading.Inline?.LastChild is { } lastContent) Hide(lastContent.Span.End + 1, end);
                }
                else
                {
                    var underline = source.LastIndexOfAny(['\r', '\n'], Math.Max(start, end - 1), end - start);
                    if (underline >= start) Hide(underline + 1, end);
                }
            }
            if (block is CodeBlock)
            {
                Add(start, end, MarkdownStyle.Code);
                if (block is FencedCodeBlock fenced)
                {
                    var firstEnd = start;
                    while (firstEnd < end && source[firstEnd] is not ('\r' or '\n')) firstEnd++;
                    Hide(start, firstEnd);
                    var lastStart = end;
                    while (lastStart > firstEnd && source[lastStart - 1] is not ('\r' or '\n')) lastStart--;
                    if (lastStart > firstEnd && source[lastStart] == fenced.FencedChar) Hide(lastStart, end);
                }
                continue;
            }
            if (block.Inline is null) continue;
            foreach (var inline in block.Inline.Descendants().OfType<Inline>())
            {
                var image = inline is LinkInline { IsImage: true };
                for (var parent = inline.Parent; parent is not null; parent = parent.Parent)
                    image |= parent is LinkInline { IsImage: true };
                if (image) continue;
                var from = inline.Span.Start; var to = inline.Span.End + 1;
                if (inline is EmphasisInline emphasis)
                {
                    Add(from, to, emphasis.DelimiterChar == '~' ? MarkdownStyle.Strike : emphasis.DelimiterCount >= 2 ? MarkdownStyle.Bold : MarkdownStyle.Italic);
                    Hide(from, from + emphasis.DelimiterCount); Hide(to - emphasis.DelimiterCount, to);
                }
                else if (inline is CodeInline)
                {
                    Add(from, to, MarkdownStyle.Code);
                    var count = 0; while (from + count < to && source[from + count] == '`') count++;
                    Hide(from, from + count); Hide(to - count, to);
                }
                else if (inline is LinkInline link && !link.IsAutoLink && link.FirstChild is { } first && link.LastChild is { } last)
                {
                    Add(from, to, MarkdownStyle.Link);
                    Hide(from, first.Span.Start); Hide(last.Span.End + 1, to);
                }
            }
        }
        return result.ToArray();
    }
}
