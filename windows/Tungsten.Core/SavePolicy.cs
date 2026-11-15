namespace Tungsten.Core;

public enum SaveBlock { None, Conflict, Missing, Uncertain }

public static class SavePolicy
{
    public static bool IsSaved(Draft draft) => draft.Body == draft.BaseBody && draft.Pending is null && draft.Block == SaveBlock.None;
    public static bool CanSend(Draft draft) => !IsSaved(draft) && draft.Pending is null && draft.Block == SaveBlock.None;
    public static Draft Begin(Draft draft)
    {
        if (!CanSend(draft)) return draft;
        Rules.Body(draft.Body);
        return draft with { Pending = new(draft.Body, draft.Revision) };
    }
    public static Draft Acknowledge(Draft current, Draft outgoing, Note note)
    {
        Scope(current, outgoing); note.Validate(outgoing.Name);
        if (outgoing.Pending is null || current.Pending != outgoing.Pending || note.Body != outgoing.Pending.Body)
            throw new InvalidDataException("Unexpected save acknowledgement.");
        return current with { BaseBody = note.Body, Revision = note.Revision, Pending = null };
    }
    public static Draft Observe(Draft current, Draft observed, Note remote)
    {
        Scope(current, observed); remote.Validate(observed.Name);
        if (remote.Revision == observed.Revision || current.Block != SaveBlock.None || current.Pending is not null) return current;
        return current.Body != observed.BaseBody ? current with { Block = SaveBlock.Conflict }
            : current with { Body = remote.Body, BaseBody = remote.Body, Revision = remote.Revision };
    }
    private static void Scope(Draft current, Draft observed)
    {
        if (current.Server != observed.Server || current.Name != observed.Name)
            throw new InvalidDataException("The response belongs to another draft.");
    }
}
