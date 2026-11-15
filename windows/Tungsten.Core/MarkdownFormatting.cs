namespace Tungsten.Core;

public sealed record MarkdownInsertion(string Text, int SelectionStart, int SelectionLength);
public static class MarkdownFormatting
{
    public static MarkdownInsertion Wrap(string source, int start, int length, MarkdownStyle style)
    {
        var selected = source.Substring(start, length);
        var from = 0; var to = selected.Length;
        if (style is MarkdownStyle.Bold or MarkdownStyle.Italic)
        {
            while (from < to && char.IsWhiteSpace(selected[from])) from++;
            while (to > from && char.IsWhiteSpace(selected[to - 1])) to--;
            if (length > 0 && from == to) return new(selected, 0, length);
        }
        var delimiter = style switch
        {
            MarkdownStyle.Bold => "**",
            MarkdownStyle.Italic => "*",
            MarkdownStyle.Strike => "~~",
            MarkdownStyle.Code => CodeDelimiter(selected),
            _ => throw new ArgumentOutOfRangeException(nameof(style))
        };
        var pad = style == MarkdownStyle.Code && (selected.StartsWith('`') || selected.EndsWith('`')) ? " " : "";
        return new(selected[..from] + delimiter + pad + selected[from..to] + pad + delimiter + selected[to..],
            from + delimiter.Length + pad.Length, to - from);
    }
    private static string CodeDelimiter(string value)
    {
        var longest = 0; var run = 0;
        foreach (var character in value) { run = character == '`' ? run + 1 : 0; longest = Math.Max(longest, run); }
        return new('`', longest + 1);
    }
}
