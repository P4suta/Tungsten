namespace Tungsten.Core;

public sealed class MarkdownSource
{
    private int[] sourceBreaks = [], nativeBreaks = [];
    public MarkdownSource(string source) => Reset(source);
    public string Source { get; private set; } = "";
    public string NativeText { get; private set; } = "";
    private void Reset(string source)
    {
        Source = source; NativeText = Normalize(source);
        var breaks = new List<int>();
        for (var index = 1; index < source.Length; index++)
            if (source[index - 1] == '\r' && source[index] == '\n') breaks.Add(index);
        sourceBreaks = breaks.ToArray(); nativeBreaks = sourceBreaks.Select((position, index) => position - index).ToArray();
    }
    public int ToSource(int offset)
    {
        ArgumentOutOfRangeException.ThrowIfNegative(offset);
        offset = Math.Min(offset, NativeText.Length);
        var index = Array.BinarySearch(nativeBreaks, offset);
        return offset + (index >= 0 ? index + 1 : ~index);
    }
    public int ToNative(int offset)
    {
        ArgumentOutOfRangeException.ThrowIfNegative(offset);
        offset = Math.Min(offset, Source.Length);
        var index = Array.BinarySearch(sourceBreaks, offset);
        return offset - (index >= 0 ? index : ~index);
    }
    public void Update(string nativeText)
    {
        var previous = NativeText;
        if (previous == nativeText) return;
        var prefix = 0;
        while (prefix < previous.Length && prefix < nativeText.Length && previous[prefix] == nativeText[prefix]) prefix++;
        var suffix = 0;
        while (suffix < previous.Length - prefix && suffix < nativeText.Length - prefix
            && previous[^(suffix + 1)] == nativeText[^(suffix + 1)]) suffix++;
        var newline = "\n";
        var first = Source.IndexOfAny(['\r', '\n']);
        if (first >= 0) newline = Source[first] == '\n' ? "\n" : first + 1 < Source.Length && Source[first + 1] == '\n' ? "\r\n" : "\r";
        var inserted = nativeText[prefix..(nativeText.Length - suffix)].Replace("\r", newline, StringComparison.Ordinal);
        Reset(Source[..ToSource(prefix)] + inserted + Source[ToSource(previous.Length - suffix)..]);
    }
    public static string Normalize(string value) => value.Replace("\r\n", "\r", StringComparison.Ordinal).Replace('\n', '\r');
}
