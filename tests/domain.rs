use tungsten::domain::{revision_matches, valid_name};

#[test]
fn accepts_flat_unicode_markdown_names() {
    for name in [
        "Note.md",
        "買い物.md",
        "A note & a #tag.md",
        "a..b.md",
        "Work/日本語.md",
    ] {
        assert!(valid_name(name.as_bytes()), "{name}");
    }
}

#[test]
fn rejects_names_that_escape_or_hide_files() {
    for name in [
        "",
        ".md",
        ".hidden.md",
        "../secret.md",
        "a/../b.md",
        "a//b.md",
        "a/.hidden/b.md",
        "/a.md",
        "a\\b.md",
        "note.txt",
        "note.MD",
        "bad\0.md",
        "bad\n.md",
        " note.md",
        "note .md",
    ] {
        assert!(!valid_name(name.as_bytes()), "{name:?}");
    }
    assert!(!valid_name(format!("{}.md", "x".repeat(238)).as_bytes()));
    assert!(valid_name(format!("{}.md", "x".repeat(237)).as_bytes()));
}

#[test]
fn updates_require_the_exact_saved_revision() {
    assert!(revision_matches(b"\"abc\"", b"\"abc\""));
    assert!(!revision_matches(b"\"abc\"", b"\"abd\""));
    assert!(!revision_matches(b"*", b"\"abc\""));
    assert!(!revision_matches(b"", b""));
}

#[test]
fn stored_paths_and_attributes_preserve_their_boundaries() {
    use tungsten::rules::{escape_attribute, safe_external_link, valid_attachment, valid_id};
    assert!(valid_name(format!("{}Note.md", "a/".repeat(15)).as_bytes()));
    assert!(!valid_name(
        format!("{}Note.md", "a/".repeat(16)).as_bytes()
    ));
    let id = "0123456789abcdef0123456789abcdef";
    assert!(valid_id(id.as_bytes()));
    assert!(valid_attachment(format!("{id}.png").as_bytes()));
    for name in ["../file.png", "invalid.png", "/absolute", "", "."] {
        assert!(!valid_attachment(name.as_bytes()));
        assert!(!valid_id(name.as_bytes()));
    }
    for uri in [
        "https://example.com",
        "http://localhost",
        "mailto:user@example.com",
    ] {
        assert!(safe_external_link(uri.as_bytes()));
    }
    for uri in [
        "javascript:alert(1)",
        "//example.com",
        "https://example.com\n",
        "data:text/html,hello",
    ] {
        assert!(!safe_external_link(uri.as_bytes()));
    }
    assert_eq!(
        escape_attribute("日本語 & <tag> \"quote\" '"),
        "日本語 &amp; &lt;tag&gt; &quot;quote&quot; &#39;"
    );
}
#[test]
fn path_validation_preserves_component_and_byte_boundaries() {
    for length in 0..=241 {
        let mut name = vec![b'a'; length];
        if length >= 3 {
            name[length - 3..].copy_from_slice(b".md");
        }
        assert_eq!(
            tungsten::rules::valid_name(&name),
            (4..=240).contains(&length)
        );
    }
    for components in 1..=17 {
        let name = format!("{}note.md", "folder/".repeat(components - 1));
        assert_eq!(
            tungsten::rules::valid_name(name.as_bytes()),
            components <= 16
        );
    }
    for invalid in [
        "a//b.md",
        "a/./b.md",
        "a/../b.md",
        "a /b.md",
        "a/ b.md",
        "a/.b.md",
        "/a.md",
        "a/.md",
        "a/ .md",
        "a/b .md",
    ] {
        assert!(
            !tungsten::rules::valid_name(invalid.as_bytes()),
            "{invalid}"
        );
    }
    for byte in 0..=255u8 {
        let name = [b'a', b'/', b'b', byte, b'c', b'.', b'm', b'd'];
        let allowed = byte >= 32 && !matches!(byte, 127 | b'\\');
        assert_eq!(tungsten::rules::valid_name(&name), allowed, "byte={byte}");
    }
}
#[test]
fn attribute_escaping_preserves_every_unicode_scalar_and_exact_entities() {
    for value in 0..=0x10ffff {
        let Some(character) = char::from_u32(value) else {
            continue;
        };
        let input = character.to_string();
        let expected = match character {
            '&' => "&amp;",
            '"' => "&quot;",
            '\'' => "&#39;",
            '<' => "&lt;",
            '>' => "&gt;",
            _ => &input,
        };
        assert_eq!(tungsten::rules::escape_attribute(&input), expected);
    }
    let input = "日本語 & <tag> \"quote\" '".repeat(1024);
    let expected = "日本語 &amp; &lt;tag&gt; &quot;quote&quot; &#39;".repeat(1024);
    assert_eq!(tungsten::rules::escape_attribute(&input), expected);
}
