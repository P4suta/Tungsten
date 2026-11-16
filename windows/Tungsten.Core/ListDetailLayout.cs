namespace Tungsten.Core;

public sealed record ListDetailLayout(bool Stacked, bool ShowNotes, bool ShowEditor, bool CanGoBack)
{
    public static ListDetailLayout For(double availableWidth, bool hasSelection)
    {
        var stacked = availableWidth <= 640;
        return new(stacked, !stacked || !hasSelection, !stacked || hasSelection, stacked && hasSelection);
    }
}
