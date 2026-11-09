use crate::{
    domain::NoteName,
    rules::{escape_attribute, safe_external_link, valid_attachment},
};
use percent_encoding::{NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use pulldown_cmark::{Alignment, Event, LinkType, Options, Parser, Tag, TagEnd, html};
use serde::Serialize;
use std::collections::{BTreeSet, HashMap, HashSet};

#[derive(Debug, Serialize)]
pub struct Heading {
    pub id: String,
    pub text: String,
    pub level: u8,
}

#[derive(Debug, Serialize)]
pub struct Task {
    pub offset: usize,
    pub checked: bool,
    pub label: String,
}

#[derive(Debug, Serialize)]
pub struct Preview {
    pub html: String,
    pub blocks: Vec<Block>,
    pub tags: Vec<String>,
    pub links: Vec<String>,
    pub outline: Vec<Heading>,
    pub tasks: Vec<Task>,
    pub words: usize,
    pub characters: usize,
}

#[derive(Debug, Serialize)]
pub struct Block {
    pub from: usize,
    pub to: usize,
    pub html: String,
}

pub fn resolve_note(source: &str, target: &str, wiki: bool) -> Option<String> {
    let decoded = percent_decode_str(target.split('#').next()?)
        .decode_utf8()
        .ok()?;
    if decoded.is_empty()
        || decoded.contains(':')
        || decoded.starts_with('/')
        || decoded.contains('\\')
    {
        return None;
    }
    if !wiki && !decoded.ends_with(".md") {
        return None;
    }
    let mut parts: Vec<&str> = source
        .rsplit_once('/')
        .map_or(Vec::new(), |(folder, _)| folder.split('/').collect());
    for part in decoded.split('/') {
        match part {
            "." => {}
            ".." => {
                parts.pop()?;
            }
            "" => return None,
            _ => parts.push(part),
        }
    }
    let mut name = parts.join("/");
    if !name.ends_with(".md") {
        name.push_str(".md");
    }
    NoteName::parse(name.clone()).ok().map(|_| name)
}

pub fn attachment_target(target: &str) -> Option<&str> {
    let mut target = target.strip_prefix('/').unwrap_or(target);
    while let Some(rest) = target
        .strip_prefix("../")
        .or_else(|| target.strip_prefix("./"))
    {
        target = rest;
    }
    let id = target
        .strip_prefix(".attachments/")?
        .split(['#', '?'])
        .next()?;
    valid_attachment(id.as_bytes()).then_some(id)
}

pub fn attachment_references(body: &str) -> Vec<String> {
    Parser::new_ext(body, Options::ENABLE_WIKILINKS)
        .filter_map(|event| match event {
            Event::Start(Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. }) => {
                attachment_target(&dest_url).map(str::to_owned)
            }
            _ => None,
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub fn retitle_links(
    source: &str,
    body: &str,
    target: &str,
    previous: &str,
    title: &str,
) -> String {
    let escape = |text: &str| {
        text.replace('\\', "\\\\")
            .replace('[', "\\[")
            .replace(']', "\\]")
    };
    let prefix = format!("[{}](", escape(previous));
    let mut ranges = Vec::new();
    for (event, range) in Parser::new(body).into_offset_iter() {
        if let Event::Start(Tag::Link {
            dest_url,
            link_type: LinkType::Inline,
            ..
        }) = event
            && resolve_note(source, &dest_url, false).as_deref() == Some(target)
            && body
                .get(range.clone())
                .is_some_and(|link| link.starts_with(&prefix))
        {
            ranges.push(range.start + 1..range.start + prefix.len() - 2);
        }
    }
    let mut updated = body.to_owned();
    let label = escape(title);
    for range in ranges.into_iter().rev() {
        updated.replace_range(range, &label);
    }
    updated
}

fn tags_in(text: &str, tags: &mut BTreeSet<String>) {
    let mut previous = ' ';
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '#' && !previous.is_alphanumeric() && previous != '/' {
            let mut tag = String::new();
            while let Some(next) = chars.peek() {
                if !next.is_alphanumeric() && !matches!(*next, '_' | '-' | '/') {
                    break;
                }
                tag.push(chars.next().unwrap_or_default());
            }
            if !tag.is_empty() && tag.chars().any(char::is_alphanumeric) {
                tags.insert(tag.trim_end_matches('/').to_lowercase());
            }
        }
        previous = character;
    }
}

fn slug(text: &str) -> String {
    let mut slug = String::new();
    for character in text.to_lowercase().chars() {
        if character.is_alphanumeric() || matches!(character, '_' | '-') {
            slug.push(character);
        } else if character.is_whitespace() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let value = slug.trim_matches('-');
    if value.is_empty() {
        "section".to_owned()
    } else {
        value.to_owned()
    }
}

fn fragment_slug(fragment: &str) -> String {
    slug(&percent_decode_str(fragment).decode_utf8_lossy())
}

fn task_label(source: &str, number: usize) -> String {
    let line = source.lines().next().unwrap_or_default().trim();
    let mut label = String::new();
    for event in Parser::new(line) {
        if let Event::Text(text) | Event::Code(text) | Event::Html(text) | Event::InlineHtml(text) =
            event
        {
            label.push_str(&text);
        }
    }
    if label.is_empty() {
        format!("Task {number}")
    } else {
        format!("Task: {label}")
    }
}

pub fn first_heading(body: &str) -> Option<String> {
    let mut title = None;
    for event in Parser::new_ext(
        body,
        Options::ENABLE_STRIKETHROUGH | Options::ENABLE_FOOTNOTES | Options::ENABLE_WIKILINKS,
    ) {
        match event {
            Event::Start(Tag::Heading { .. }) => title = Some(String::new()),
            Event::Text(text) | Event::Code(text) => {
                if let Some(title) = &mut title {
                    title.push_str(&text);
                }
            }
            Event::End(TagEnd::Heading(_)) => return title,
            _ => {}
        }
    }
    None
}

pub fn preview(name: &str, body: &str) -> Preview {
    let mut utf16 = vec![0; body.len() + 1];
    let mut units = 0;
    for (offset, character) in body.char_indices() {
        utf16[offset] = units;
        units += character.len_utf16();
    }
    utf16[body.len()] = units;
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_WIKILINKS
        | Options::ENABLE_GFM;
    let events: Vec<_> = Parser::new_ext(body, options).into_offset_iter().collect();
    let mut tags = BTreeSet::new();
    let mut links = BTreeSet::new();
    let mut outline: Vec<Heading> = Vec::new();
    let mut heading_ids = HashSet::new();
    let mut heading_suffixes = HashMap::<String, usize>::new();
    let mut tasks = Vec::new();
    let mut heading = None;
    let mut in_code = false;
    for (event, range) in &events {
        match event {
            Event::Start(Tag::CodeBlock(_)) => in_code = true,
            Event::End(TagEnd::CodeBlock) => in_code = false,
            Event::Start(Tag::Heading { level, .. }) => {
                heading = Some((*level as u8, String::new()));
            }
            Event::Text(text) if !in_code => {
                tags_in(text, &mut tags);
                if let Some((_, text_so_far)) = &mut heading {
                    text_so_far.push_str(text);
                }
            }
            Event::Code(text) => {
                if let Some((_, text_so_far)) = &mut heading {
                    text_so_far.push_str(text);
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some((level, text)) = heading.take() {
                    let base = format!("md-{}", slug(&text));
                    let mut id = base.clone();
                    let suffix = heading_suffixes.entry(base.clone()).or_default();
                    while !heading_ids.insert(id.clone()) {
                        *suffix += 1;
                        id = format!("{base}-{suffix}");
                    }
                    outline.push(Heading { id, text, level });
                }
            }
            Event::Start(Tag::Link {
                dest_url,
                link_type,
                ..
            }) => {
                if let Some(target) = resolve_note(
                    name,
                    dest_url,
                    matches!(link_type, LinkType::WikiLink { .. }),
                ) {
                    links.insert(target);
                }
            }
            Event::TaskListMarker(checked) => tasks.push(Task {
                offset: utf16[range.start],
                checked: *checked,
                label: task_label(&body[range.end..], tasks.len() + 1),
            }),
            _ => {}
        }
    }
    let mut rendered = Vec::new();
    let mut heading_index = 0;
    let mut task_index = 0;
    let mut blocked_links = Vec::new();
    let mut blocked_images = Vec::new();
    let mut table_alignment = Vec::new();
    let mut table_column = 0;
    let mut table_head = false;
    let mut blocks = Vec::new();
    let mut depth = 0usize;
    let mut block_start = 0;
    let mut rendered_start = 0;
    for (event, range) in events {
        if depth == 0 {
            block_start = range.start;
            rendered_start = rendered.len();
        }
        match &event {
            Event::Start(_) => depth += 1,
            Event::End(_) => depth = depth.saturating_sub(1),
            _ => {}
        }
        let event = match event {
            Event::SoftBreak => Event::HardBreak,
            Event::Start(Tag::Table(alignment)) => {
                table_alignment.clone_from(&alignment);
                rendered.push(Event::Html("<div class=\"table-scroll\">".into()));
                Event::Start(Tag::Table(alignment))
            }
            event @ Event::Start(Tag::TableHead) => {
                table_head = true;
                table_column = 0;
                event
            }
            event @ Event::End(TagEnd::TableHead) => {
                table_head = false;
                event
            }
            event @ Event::Start(Tag::TableRow) => {
                table_column = 0;
                event
            }
            Event::Start(Tag::TableCell) => {
                let tag = if table_head { "th" } else { "td" };
                let alignment = match table_alignment.get(table_column) {
                    Some(Alignment::Left) => " class=\"align-left\"",
                    Some(Alignment::Center) => " class=\"align-center\"",
                    Some(Alignment::Right) => " class=\"align-right\"",
                    _ => "",
                };
                table_column += 1;
                Event::Html(format!("<{tag}{alignment}>").into())
            }
            Event::End(TagEnd::Table) => {
                rendered.push(Event::End(TagEnd::Table));
                Event::Html("</div>".into())
            }
            Event::Html(raw) | Event::InlineHtml(raw) => Event::Text(raw),
            Event::Start(Tag::Heading { level, .. }) => {
                let id = outline[heading_index].id.clone();
                heading_index += 1;
                Event::Start(Tag::Heading {
                    level,
                    id: Some(id.into()),
                    classes: Vec::new(),
                    attrs: Vec::new(),
                })
            }
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                let destination = if let Some(target) = resolve_note(
                    name,
                    &dest_url,
                    matches!(link_type, LinkType::WikiLink { .. }),
                ) {
                    let fragment = dest_url
                        .split_once('#')
                        .map_or(String::new(), |(_, value)| {
                            format!(
                                "#md-{}",
                                utf8_percent_encode(&fragment_slug(value), NON_ALPHANUMERIC)
                            )
                        });
                    Some(format!(
                        "/?note={}{}",
                        utf8_percent_encode(&target, NON_ALPHANUMERIC),
                        fragment
                    ))
                } else if let Some(asset) = attachment_target(&dest_url) {
                    Some(format!("/.attachments/{asset}"))
                } else if let Some(fragment) = dest_url.strip_prefix('#') {
                    Some(format!("#md-{}", fragment_slug(fragment)))
                } else if safe_external_link(dest_url.to_ascii_lowercase().as_bytes()) {
                    Some(dest_url.to_string())
                } else {
                    None
                };
                blocked_links.push(destination.is_none());
                match destination {
                    Some(dest_url) => Event::Start(Tag::Link {
                        link_type,
                        dest_url: dest_url.into(),
                        title,
                        id,
                    }),
                    None => Event::Html("<span>".into()),
                }
            }
            Event::End(TagEnd::Link) if blocked_links.pop().unwrap_or(false) => {
                Event::Html("</span>".into())
            }
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                if let Some(asset) = attachment_target(&dest_url) {
                    blocked_images.push(false);
                    Event::Start(Tag::Image {
                        link_type,
                        dest_url: format!("/.attachments/{asset}").into(),
                        title,
                        id,
                    })
                } else {
                    blocked_images.push(true);
                    Event::Text("[Image: ".into())
                }
            }
            Event::End(TagEnd::Image) if blocked_images.pop().unwrap_or(false) => {
                Event::Text("]".into())
            }
            Event::TaskListMarker(checked) => {
                let check = if checked { " checked" } else { "" };
                let label = escape_attribute(&tasks[task_index].label);
                task_index += 1;
                Event::Html(
                    format!(
                        "<label class=\"task-control\"><input type=\"checkbox\" aria-label=\"{label}\" data-task=\"{}\"{check}></label>",
                        task_index - 1
                    )
                    .into(),
                )
            }
            event => event,
        };
        rendered.push(event);
        if depth == 0 && range.end > block_start {
            let mut html = String::new();
            html::push_html(&mut html, rendered[rendered_start..].iter().cloned());
            blocks.push(Block {
                from: utf16[block_start],
                to: utf16[range.end],
                html,
            });
        }
    }
    let mut html = String::new();
    html::push_html(&mut html, rendered.into_iter());
    Preview {
        html,
        blocks,
        tags: tags.into_iter().collect(),
        links: links.into_iter().collect(),
        outline,
        tasks,
        words: body.split_whitespace().count(),
        characters: body.chars().count(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_headings_and_tasks_keep_unique_ids_and_utf16_offsets() {
        let body = format!(
            "# A\n# A-1\n# A\n{}{}",
            "## 同じ 😀\n\n".repeat(20_000),
            "- [ ] 日本語 😀\n".repeat(10_000)
        );
        let rendered = preview("Note.md", &body);
        assert_eq!(rendered.outline.len(), 20_003);
        assert_eq!(rendered.outline[0].id, "md-a");
        assert_eq!(rendered.outline[1].id, "md-a-1");
        assert_eq!(rendered.outline[2].id, "md-a-2");
        assert_eq!(
            rendered
                .outline
                .iter()
                .map(|heading| &heading.id)
                .collect::<HashSet<_>>()
                .len(),
            rendered.outline.len()
        );
        let utf16 = body.encode_utf16().collect::<Vec<_>>();
        assert_eq!(rendered.tasks.len(), 10_000);
        for task in rendered.tasks {
            assert_eq!(&utf16[task.offset..task.offset + 3], &[91, 32, 93]);
        }
        assert_eq!(rendered.blocks.last().unwrap().to, utf16.len());
    }

    #[test]
    fn single_newlines_render_like_github_comments_without_changing_code_or_paragraphs() {
        let rendered = preview(
            "Note.md",
            "One [link](https://example.com)\nTwo\n\nThree  \nFour\\\nFive\n\n```txt\na\nb\n```\n",
        );
        assert!(rendered.html.contains("</a><br />\nTwo</p>"));
        assert!(rendered.html.contains("Three<br />\nFour<br />\nFive</p>"));
        assert!(
            rendered
                .html
                .contains("<code class=\"language-txt\">a\nb\n</code>")
        );
        assert_eq!(rendered.html.matches("<p>").count(), 2);
    }

    #[test]
    fn rendered_blocks_keep_unicode_offsets_and_reference_links() {
        let body = "# 日本語😀\n\nA **bold** [link][target].\n\n[target]: Target.md\n";
        let result = preview("Source.md", body);
        assert_eq!(result.blocks.len(), 2);
        let encoded: Vec<_> = body.encode_utf16().collect();
        for block in &result.blocks {
            assert!(block.from < block.to && block.to <= encoded.len());
            assert!(String::from_utf16(&encoded[block.from..block.to]).is_ok());
        }
        assert!(result.blocks[1].html.contains("<strong>bold</strong>"));
        assert!(result.blocks[1].html.contains("note=Target%2Emd"));
        assert_eq!(
            result
                .blocks
                .iter()
                .map(|block| block.html.as_str())
                .collect::<String>(),
            result.html
        );
        let hostile = preview(
            "Source.md",
            "<script>bad()</script>\n\n[bad](javascript:evil)",
        );
        assert!(
            hostile
                .blocks
                .iter()
                .all(|block| !block.html.contains("<script>")
                    && !block.html.contains("href=\"javascript:"))
        );
    }

    #[test]
    fn table_alignment_uses_stylesheet_classes() {
        let result = preview(
            "Note.md",
            "| Left | Center | Right | Default |\n| :--- | :---: | ---: | --- |\n| A | B | C | D |",
        );
        assert!(result.html.contains("<th class=\"align-left\">Left</th>"));
        assert!(
            result
                .html
                .contains("<th class=\"align-center\">Center</th>")
        );
        assert!(result.html.contains("<th class=\"align-right\">Right</th>"));
        assert!(result.html.contains("<th>Default</th>"));
        assert!(result.html.contains("<td class=\"align-left\">A</td>"));
        assert!(result.html.contains("<td class=\"align-center\">B</td>"));
        assert!(result.html.contains("<td class=\"align-right\">C</td>"));
        assert!(result.html.contains("<td>D</td>"));
        assert!(!result.html.contains("style="));
    }

    #[test]
    fn heading_ids_do_not_collide_with_editor_controls() {
        let result = preview("Note.md", "# body\n\n[jump](#body)");
        assert_eq!(result.outline[0].id, "md-body");
        assert!(result.html.contains("href=\"#md-body\""));
        let unicode = preview(
            "Note.md",
            "# 日本語\n\n[jump](#%E6%97%A5%E6%9C%AC%E8%AA%9E)",
        );
        assert!(unicode.html.contains("id=\"md-日本語\""));
        assert!(
            unicode
                .html
                .contains("href=\"#md-%E6%97%A5%E6%9C%AC%E8%AA%9E\"")
        );
    }

    #[test]
    fn hostile_links_and_code_tags_stay_inert() {
        for url in [
            "javascript:alert(1)",
            "data:text/html,hi",
            "//tracker.test/a",
            "file:///tmp/a",
            "jAvAsCrIpT:alert(1)",
        ] {
            let result = preview("Note.md", &format!("[link]({url})"));
            assert!(!result.html.contains("href="), "{url}");
        }
        let result = preview(
            "Note.md",
            "# Heading\n\n#work #日本語\n\n`#inline`\n\n```\n#code\n```\n\n# Heading",
        );
        assert_eq!(result.tags, ["work", "日本語"]);
        assert_eq!(result.outline[1].id, "md-heading-1");
        assert!(resolve_note("Note.md", "../../Escape", true).is_none());
    }
}
