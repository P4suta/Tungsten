using Tungsten.Core;

namespace Tungsten.Tests;

public sealed class MarkdownLinkTests
{
    [Fact]
    public void DetailsUseReadableLabelsWhileRetainingNavigationIdentities()
    {
        var origin = new Uri("https://notes.example/");
        var attachment = new Attachment(new string('a', 32) + ".txt", "[original].txt", 1, "\"" + new string('a', 64) + "\"");
        var body = MarkdownLinks.Note("folder/source.md", new("target.md", "Planning [notes]")) + "\n"
            + MarkdownLinks.Original("folder/source.md", attachment);
        var rows = MarkdownLinks.References(origin, "folder/source.md", body, new("folder/source.md", ["target.md"], [attachment.Id]));
        Assert.Equal(new NoteReference("target.md", "Planning [notes]"), Assert.Single(rows.Links));
        Assert.Equal(attachment.Id, Assert.Single(rows.Attachments).Id);
        Assert.Equal(attachment.Name, Assert.Single(rows.Attachments).Text);
    }

    [Fact]
    public void DetailLabelsRespectOriginsFragmentsFormattingAndUnlabelledReferences()
    {
        var body = "[Wrong](https://other.example/target.md)\n[**Planning** `notes` &amp; links](../target.md#section)";
        var rows = MarkdownLinks.References(new("https://notes.example/"), "folder/source.md", body,
            new("folder/source.md", ["target.md", "unlabelled.md"], []));
        Assert.Equal("Planning notes & links", rows.Links[0].Text);
        Assert.Equal("unlabelled.md", rows.Links[1].Text);
    }

    [Theory]
    [InlineData("source.md", "target.md", "Target", "[Target](target.md)")]
    [InlineData("folder/source.md", "target.md", "Target", "[Target](../target.md)")]
    [InlineData("folder/source.md", "folder/a #.md", "A [label]", "[A \\[label\\]](a%20%23.md)")]
    [InlineData("a/b/source.md", "a/c/target.md", "Back\\slash", "[Back\\\\slash](../c/target.md)")]
    public void NoteLinksArePortableRelativeMarkdown(string source, string target, string title, string expected)
        => Assert.Equal(expected, MarkdownLinks.Note(source, new(target, title)));

    [Fact]
    public void OriginalLinksReachTheSharedAttachmentDirectoryFromNestedNotes()
    {
        var attachment = new Attachment(new string('a', 32) + ".txt", "[original].txt", 1, "\"" + new string('a', 64) + "\"");
        Assert.Equal("[\\[original\\].txt](../../.attachments/" + attachment.Id + ")", MarkdownLinks.Original("a/b/note.md", attachment));
    }
}
