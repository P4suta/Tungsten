namespace Tungsten.Core;

public enum FocusRegion { Collections, Notes, Editor }

public static class FocusNavigation
{
    public static FocusRegion Move(FocusRegion current, bool reverse, bool notesVisible, bool editorVisible)
    {
        List<FocusRegion> regions = [FocusRegion.Collections];
        if (notesVisible) regions.Add(FocusRegion.Notes);
        if (editorVisible) regions.Add(FocusRegion.Editor);
        var index = regions.IndexOf(current);
        return index < 0 ? regions[0] : regions[(index + (reverse ? -1 : 1) + regions.Count) % regions.Count];
    }
}
