using Markdig.Syntax;
using Markdig.Syntax.Inlines;

namespace Tungsten.Core;

public sealed record NoteReference(string Name, string Text) { public override string ToString() => Text; }
public sealed record AttachmentReference(string Id, string Text) { public override string ToString() => Text; }
public sealed record NoteReferences(NoteReference[] Links, AttachmentReference[] Attachments);

public static class MarkdownLinks
{
    public static NoteReferences References(Uri origin, string source, string body, Connections connections)
    {
        Rules.NoteName(source); Rules.LocalBody(body);
        if (connections.Name != source || connections.Links is null || connections.Attachments is null)
            throw new InvalidDataException("Unexpected note references.");
        origin = Rules.Origin(origin.AbsoluteUri, true);
        var address = new Uri(origin, string.Join('/', source.Split('/').Select(Uri.EscapeDataString)));
        var labels = new Dictionary<string, string>(StringComparer.Ordinal);
        string Raw(Inline value) => value.Span.Start >= 0 && value.Span.End >= value.Span.Start && value.Span.End < body.Length
            ? body[value.Span.Start..(value.Span.End + 1)] : "";
        string Children(ContainerInline container)
        {
            var text = new System.Text.StringBuilder();
            for (var child = container.FirstChild; child is not null; child = child.NextSibling) text.Append(Text(child));
            return text.ToString();
        }
        string Text(Inline value) => value switch
        {
            LiteralInline literal => literal.Content.ToString(),
            CodeInline code => code.Content,
            HtmlEntityInline => System.Net.WebUtility.HtmlDecode(Raw(value)),
            LineBreakInline => " ",
            ContainerInline container => Children(container),
            _ => Raw(value)
        };
        foreach (var block in LiveMarkdown.Parse(body).Descendants().OfType<LeafBlock>())
        {
            if (block.Inline is null) continue;
            foreach (var link in block.Inline.Descendants().OfType<LinkInline>())
            {
                if (!Uri.TryCreate(address, link.Url, out var target) || target.Scheme != origin.Scheme
                    || target.GetLeftPart(UriPartial.Authority) != origin.GetLeftPart(UriPartial.Authority)) continue;
                var label = Text(link).Trim();
                if (label.Length > 0) labels.TryAdd(Uri.UnescapeDataString(target.AbsolutePath).TrimStart('/'), label);
            }
        }
        return new(connections.Links.Select(name => new NoteReference(Rules.NoteName(name), labels.GetValueOrDefault(name, name))).ToArray(),
            connections.Attachments.Select(id => new AttachmentReference(Rules.AttachmentId(id), labels.GetValueOrDefault(".attachments/" + id, id))).ToArray());
    }

    private static string Label(string value) => value.Replace("\\", "\\\\", StringComparison.Ordinal)
        .Replace("[", "\\[", StringComparison.Ordinal).Replace("]", "\\]", StringComparison.Ordinal)
        .Replace('\r', ' ').Replace('\n', ' ');
    public static string Note(string source, Summary target)
    {
        var parent = Rules.NoteName(source).Split('/')[..^1];
        var destination = Rules.NoteName(target.Name).Split('/');
        var common = 0;
        while (common < parent.Length && common < destination.Length - 1 && parent[common] == destination[common]) common++;
        var path = string.Concat(Enumerable.Repeat("../", parent.Length - common))
            + string.Join('/', destination[common..].Select(Uri.EscapeDataString));
        return $"[{Label(target.Title)}]({path})";
    }
    public static string Original(string source, Attachment attachment)
    {
        var prefix = string.Concat(Enumerable.Repeat("../", Rules.NoteName(source).Count(c => c == '/')));
        return $"[{Label(attachment.Name)}]({prefix}.attachments/{Rules.AttachmentId(attachment.Id)})";
    }
}
