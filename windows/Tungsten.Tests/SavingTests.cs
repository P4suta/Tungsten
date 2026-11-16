using Tungsten.Core;

namespace Tungsten.Tests;

public sealed class SavingTests
{
    private static readonly Uri Server = new("https://notes.example/");
    private static readonly string Revision = "\"" + new string('a', 64) + "\"";
    private static readonly string Changed = "\"" + new string('b', 64) + "\"";
    private static Draft Loaded() => Draft.From(Server, new("a.md", "before", Revision));

    [Fact]
    public void OnlyDirtyIdleUnblockedDraftsMayBeginSending()
    {
        Assert.True(SavePolicy.IsSaved(Loaded()));
        Assert.False(SavePolicy.CanSend(Loaded()));
        var sending = SavePolicy.Begin(Loaded() with { Body = "sent" });
        Assert.Equal(new PendingWrite("sent", Revision), sending.Pending);
        Assert.False(SavePolicy.CanSend(sending));
        Assert.Equal(sending, SavePolicy.Begin(sending));
    }

    [Fact]
    public void AcknowledgementPreservesLaterTextAndAdvancesTheBaseRevision()
    {
        var outgoing = Loaded() with { Body = "sent", Pending = new("sent", Revision) };
        var current = outgoing with { Body = "later" };
        var saved = SavePolicy.Acknowledge(current, outgoing, new("a.md", "sent", Changed));
        Assert.Equal("later", saved.Body); Assert.Equal("sent", saved.BaseBody);
        Assert.Equal(Changed, saved.Revision); Assert.Null(saved.Pending);
        Assert.True(SavePolicy.CanSend(saved)); Assert.False(SavePolicy.IsSaved(saved));
    }

    [Theory]
    [InlineData(SaveBlock.Conflict)]
    [InlineData(SaveBlock.Missing)]
    [InlineData(SaveBlock.Uncertain)]
    public void FailuresBlockAutomaticWritesUntilExplicitRecovery(SaveBlock block)
    {
        var draft = Loaded() with { Body = "local", Block = block };
        Assert.False(SavePolicy.CanSend(draft)); Assert.False(SavePolicy.IsSaved(draft));
        Assert.Equal(draft, SavePolicy.Begin(draft));
    }

    [Fact]
    public void AcknowledgementsFromAnotherScopeOrWriteAreRejected()
    {
        var outgoing = Loaded() with { Body = "sent", Pending = new("sent", Revision) };
        var note = new Note("a.md", "sent", Changed);
        Assert.Throws<InvalidDataException>(() => SavePolicy.Acknowledge(outgoing with { Name = "b.md" }, outgoing, note));
        Assert.Throws<InvalidDataException>(() => SavePolicy.Acknowledge(outgoing with { Server = "https://other.example/" }, outgoing, note));
        Assert.Throws<InvalidDataException>(() => SavePolicy.Acknowledge(outgoing with { Pending = new("another", Revision) }, outgoing, note));
        Assert.Throws<InvalidDataException>(() => SavePolicy.Acknowledge(outgoing, outgoing, note with { Body = "unexpected" }));
    }

    [Fact]
    public void RemoteChangesRefreshCleanTextAndProtectDirtyText()
    {
        var observed = Loaded(); var remote = new Note("a.md", "remote", Changed);
        var clean = SavePolicy.Observe(observed, observed, remote);
        Assert.Equal("remote", clean.Body); Assert.True(SavePolicy.IsSaved(clean));
        var dirty = SavePolicy.Observe(observed with { Body = "later input" }, observed, remote);
        Assert.Equal("later input", dirty.Body); Assert.Equal(SaveBlock.Conflict, dirty.Block);
    }
}
