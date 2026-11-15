namespace Tungsten.Core;

public sealed record MarkdownInsertion(string Text, int SelectionStart, int SelectionLength);
public static class MarkdownFormatting
{
    public static MarkdownInsertion Wrap(string source, int start, int length, MarkdownStyle style)
    {
        var selected = source.Substring(start, length);
        var delimiter = style switch
        {
            MarkdownStyle.Bold => "**",
            MarkdownStyle.Italic => "*",
            MarkdownStyle.Strike => "~~",
            MarkdownStyle.Code => CodeDelimiter(selected),
            _ => throw new ArgumentOutOfRangeException(nameof(style))
        };
        var pad = style == MarkdownStyle.Code && (selected.StartsWith('`') || selected.EndsWith('`')) ? " " : "";
        return new(delimiter + pad + selected + pad + delimiter, delimiter.Length + pad.Length, length);
    }
    private static string CodeDelimiter(string value)
    {
        var longest = 0; var run = 0;
        foreach (var character in value) { run = character == '`' ? run + 1 : 0; longest = Math.Max(longest, run); }
        return new('`', longest + 1);
    }
}
