namespace Tungsten.Core;

public sealed record TextSelection(int Start, int Length);
public sealed record MarkdownSnapshot(string Source, TextSelection Selection);
public sealed class MarkdownHistory(string source)
{
    private sealed record Edit(int Start, string Removed, string Inserted, TextSelection Before, TextSelection After);
    private readonly List<Edit> undo = [];
    private readonly List<Edit> redo = [];
    private MarkdownSnapshot? composition;
    private bool merging;
    public string Source { get; private set; } = source;
    public TextSelection Selection { get; private set; } = new(0, 0);
    public bool CanUndo => composition is null && undo.Count > 0;
    public bool CanRedo => composition is null && redo.Count > 0;
    private static void Validate(TextSelection selection, int length)
    {
        if (selection.Start < 0 || selection.Length < 0 || selection.Start > length - selection.Length)
            throw new ArgumentOutOfRangeException(nameof(selection));
    }
    public void Select(TextSelection selection)
    {
        Validate(selection, Source.Length);
        if (selection != Selection) BreakGroup();
        Selection = selection;
    }
    public void Record(string value, TextSelection selection, bool coalesce = true)
    {
        Validate(selection, value.Length);
        if (value == Source) { Selection = selection; return; }
        if (composition is not null) { Source = value; Selection = selection; return; }
        var prefix = 0;
        while (prefix < Source.Length && prefix < value.Length && Source[prefix] == value[prefix]) prefix++;
        var suffix = 0;
        while (suffix < Source.Length - prefix && suffix < value.Length - prefix && Source[^(suffix + 1)] == value[^(suffix + 1)]) suffix++;
        var edit = new Edit(prefix, Source[prefix..(Source.Length - suffix)], value[prefix..(value.Length - suffix)], Selection, selection);
        redo.Clear();
        var merged = false;
        if (coalesce && merging && undo.LastOrDefault() is { } previous)
        {
            if (previous.Removed.Length == 0 && edit.Removed.Length == 0 && edit.Start == previous.Start + previous.Inserted.Length
                && !edit.Inserted.Contains('\n') && !edit.Inserted.Contains('\r'))
            { undo[^1] = previous with { Inserted = previous.Inserted + edit.Inserted, After = selection }; merged = true; }
            else if (previous.Inserted.Length == 0 && edit.Inserted.Length == 0 && edit.Start + edit.Removed.Length == previous.Start)
            { undo[^1] = previous with { Start = edit.Start, Removed = edit.Removed + previous.Removed, After = selection }; merged = true; }
        }
        if (!merged) undo.Add(edit);
        Source = value; Selection = selection; merging = coalesce;
        long retained = undo.Sum(e => (long)e.Removed.Length + e.Inserted.Length);
        while (undo.Count > 256 || retained > 16 * 1024 * 1024)
        { retained -= undo[0].Removed.Length + undo[0].Inserted.Length; undo.RemoveAt(0); }
    }
    public void BeginComposition()
    {
        if (composition is not null) return;
        BreakGroup(); composition = new(Source, Selection);
    }
    public void EndComposition()
    {
        if (composition is not { } before) return;
        var after = new MarkdownSnapshot(Source, Selection);
        composition = null; Source = before.Source; Selection = before.Selection;
        Record(after.Source, after.Selection, false);
    }
    public void BreakGroup() => merging = false;
    public void AmendSelection(TextSelection selection)
    {
        Select(selection);
        if (composition is null && redo.Count == 0 && undo.Count > 0) undo[^1] = undo[^1] with { After = selection };
    }
    public MarkdownSnapshot Undo()
    {
        BreakGroup();
        if (CanUndo)
        {
            var edit = undo[^1]; undo.RemoveAt(undo.Count - 1); redo.Add(edit);
            Source = Source[..edit.Start] + edit.Removed + Source[(edit.Start + edit.Inserted.Length)..]; Selection = edit.Before;
        }
        return new(Source, Selection);
    }
    public MarkdownSnapshot Redo()
    {
        BreakGroup();
        if (CanRedo)
        {
            var edit = redo[^1]; redo.RemoveAt(redo.Count - 1); undo.Add(edit);
            Source = Source[..edit.Start] + edit.Inserted + Source[(edit.Start + edit.Removed.Length)..]; Selection = edit.After;
        }
        return new(Source, Selection);
    }
}
