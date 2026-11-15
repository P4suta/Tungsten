using Tungsten.Core;

namespace Tungsten.Tests;

public sealed class ValidationTests
{
    [Theory]
    [InlineData("a.md")]
    [InlineData("日本語/文章.md")]
    [InlineData("notes/one two.md")]
    public void NoteNamesPreserveValidServerIdentities(string name) => Assert.Equal(name, Rules.NoteName(name));

    [Theory]
    [InlineData("../escape.md")]
    [InlineData("/root.md")]
    [InlineData("a\\b.md")]
    [InlineData("a//b.md")]
    [InlineData(".hidden.md")]
    [InlineData("a/ b.md")]
    [InlineData("a .md")]
    [InlineData("a.txt")]
    [InlineData("a\n.md")]
    public void InvalidNoteNamesAreRejected(string name) => Assert.Throws<InvalidDataException>(() => Rules.NoteName(name));

    [Fact]
    public void NoteNameLimitsUseUtf8BytesAndSixteenComponents()
    {
        Assert.Equal(240, Rules.Utf8.GetByteCount(Rules.NoteName(new string('a', 237) + ".md")));
        Assert.Throws<InvalidDataException>(() => Rules.NoteName(new string('a', 238) + ".md"));
        Assert.Throws<InvalidDataException>(() => Rules.NoteName(new string('語', 80) + ".md"));
        Assert.NotEmpty(Rules.NoteName(string.Join('/', Enumerable.Repeat("a", 16)) + ".md"));
        Assert.Throws<InvalidDataException>(() => Rules.NoteName(string.Join('/', Enumerable.Repeat("a", 17)) + ".md"));
    }

    [Fact]
    public void RevisionsAndRecordAttachmentWorkspaceIdentitiesFollowServerRules()
    {
        var id = new string('a', 32); var revision = "\"" + new string('A', 64) + "\"";
        Assert.Equal(revision, Rules.Revision(revision));
        Assert.Equal(id, Rules.RecordId(id));
        Assert.Equal(id + ".png", Rules.AttachmentId(id + ".png"));
        Assert.Equal("9223372036854775807", Rules.WorkspaceId("9223372036854775807"));
        foreach (var bad in new[] { "1", "\"hash\"", revision + "\n" }) Assert.Throws<InvalidDataException>(() => Rules.Revision(bad));
        foreach (var bad in new[] { "0", "01", "-1", "9223372036854775808" }) Assert.Throws<InvalidDataException>(() => Rules.WorkspaceId(bad));
        Assert.Throws<InvalidDataException>(() => Rules.RecordId(id.ToUpperInvariant()));
        Assert.Throws<InvalidDataException>(() => Rules.AttachmentId(id + ".../exe"));
    }

    [Theory]
    [InlineData("https://NOTES.example:443/", "https://notes.example/")]
    [InlineData("https://notes.example:8443", "https://notes.example:8443/")]
    public void OriginsAreCanonical(string input, string expected) => Assert.Equal(expected, Rules.Origin(input).AbsoluteUri);

    [Theory]
    [InlineData("http://notes.example")]
    [InlineData("https://user@notes.example")]
    [InlineData("https://notes.example/path")]
    [InlineData("https://notes.example/?q")]
    [InlineData("https://notes.example/#fragment")]
    [InlineData("https://notes.example:0")]
    [InlineData("https://notes.example/../")]
    public void UntrustedOriginsAreRejected(string input) => Assert.Throws<InvalidDataException>(() => Rules.Origin(input));

    [Fact]
    public void DebugHttpIsLimitedToLoopback()
    {
        Assert.Equal("http://127.0.0.1:8080/", Rules.Origin("http://127.0.0.1:8080", true).AbsoluteUri);
        Assert.Throws<InvalidDataException>(() => Rules.Origin("http://127.0.0.1:8080"));
        Assert.Throws<InvalidDataException>(() => Rules.Origin("http://192.0.2.1", true));
    }

    [Fact]
    public void NoteBodiesAreLimitedByUtf8Bytes()
    {
        Rules.Body(new string('a', Rules.MaxNoteBytes));
        Assert.Throws<InvalidDataException>(() => Rules.Body(new string('a', Rules.MaxNoteBytes + 1)));
        Assert.Throws<InvalidDataException>(() => Rules.Body(new string('語', Rules.MaxNoteBytes / 3 + 1)));
    }

    [Fact]
    public void DraftAndNoteValidationRejectInconsistentRecoveryData()
    {
        var server = new Uri("https://notes.example/");
        var fresh = Draft.New(server).Validate();
        Assert.Null(fresh.Revision); Assert.Null(fresh.BaseBody); Rules.NoteName(fresh.Name);
        Assert.Equal("Untitled Note", Rules.Title("")); Assert.Equal("Heading", Rules.Title("# Heading\nbody"));
        var note = new Note("a.md", "# Heading", "\"" + new string('a', 64) + "\"");
        Assert.Equal(note, note.Validate("a.md"));
        Assert.Throws<InvalidDataException>(() => note.Validate("b.md"));
        var draft = Draft.From(server, note).Validate();
        Assert.Equal(note.Body, draft.BaseBody);
        Assert.Throws<InvalidDataException>(() => (draft with { Format = 2 }).Validate());
        Assert.Throws<InvalidDataException>(() => (draft with { BaseBody = null }).Validate());
        Assert.Throws<InvalidDataException>(() => (draft with { Pending = new("pending", "wrong") }).Validate());
    }
}
