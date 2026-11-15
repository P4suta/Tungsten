namespace Tungsten.Core;

public static class NoteList
{
    public static Summary[] Merge(IEnumerable<Summary> existing, IEnumerable<Summary> incoming)
    {
        List<Summary> rows = []; Dictionary<string, int> indices = new(StringComparer.Ordinal);
        foreach (var row in existing.Concat(incoming))
        {
            if (indices.TryGetValue(row.Name, out var index)) rows[index] = row;
            else { indices.Add(row.Name, rows.Count); rows.Add(row); }
        }
        return rows.ToArray();
    }
}
