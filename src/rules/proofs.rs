use super::{
    attribute_segments, revision_matches, safe_external_link, valid_attachment, valid_id,
    valid_name,
};

#[kani::proof]
#[kani::unwind(242)]
fn accepted_names_stay_relative_and_visible() {
    let bytes: [u8; 240] = kani::any();
    let length: u8 = kani::any();
    kani::assume(usize::from(length) <= bytes.len());
    let length = usize::from(length);
    let name = &bytes[..length];
    let accepted = valid_name(name);
    if accepted {
        assert!(length >= 4);
        assert!(name.ends_with(b".md"));
        let selected: u8 = kani::any();
        let selected = usize::from(selected);
        if selected < length {
            let byte = name[selected];
            assert!(!matches!(byte, 0..=31 | 127 | b'\\'));
            if selected == 0 || name[selected - 1] == b'/' {
                assert!(!matches!(byte, b'.' | b' ' | b'/'));
            }
            if byte == b'/' {
                assert!(selected > 0 && name[selected - 1] != b' ');
                assert!(selected + 1 < length);
            }
        }
    }
    kani::cover!(accepted, "relative visible note paths are accepted");
    kani::cover!(!accepted, "invalid note paths are refused");
}

#[kani::proof]
#[kani::unwind(242)]
fn accepted_names_have_at_most_sixteen_components() {
    let bytes: [u8; 240] = kani::any();
    let length: u8 = kani::any();
    kani::assume(usize::from(length) <= bytes.len());
    let length = usize::from(length);
    let name = &bytes[..length];
    let accepted = valid_name(name);
    let mut components = 1u8;
    if accepted {
        let mut index = 0;
        while index < length {
            components += u8::from(name[index] == b'/');
            index += 1;
        }
        assert!(components <= 16);
    }
    kani::cover!(
        accepted && components == 16,
        "the maximum directory depth remains accepted"
    );
    kani::cover!(!accepted, "invalid note paths remain refused");
}

#[kani::proof]
#[kani::unwind(47)]
fn record_names_cannot_traverse_directories() {
    let bytes: [u8; 45] = kani::any();
    let length: u8 = kani::any();
    kani::assume(usize::from(length) <= bytes.len());
    let length = usize::from(length);
    let name = &bytes[..length];
    let internal = valid_id(name);
    let attachment = valid_attachment(name);
    if internal || attachment {
        let selected: u8 = kani::any();
        if usize::from(selected) < length {
            assert!(!matches!(name[usize::from(selected)], b'/' | b'\\'));
        }
        assert_ne!(name[0], b'.');
    }
    kani::cover!(internal, "internal note identifiers are accepted");
    kani::cover!(attachment, "original file identifiers are accepted");
    kani::cover!(
        !internal && !attachment,
        "malformed identifiers are refused"
    );
}

#[kani::proof]
#[kani::unwind(68)]
fn revision_check_accepts_only_identical_bytes() {
    let expected: [u8; 66] = kani::any();
    let current: [u8; 66] = kani::any();
    let expected_length: usize = kani::any();
    let current_length: usize = kani::any();
    kani::assume(expected_length <= expected.len() && current_length <= current.len());
    let expected = &expected[..expected_length];
    let current = &current[..current_length];
    let accepted = revision_matches(expected, current);
    assert_eq!(accepted, !expected.is_empty() && expected == current);
    kani::cover!(accepted, "identical nonempty revision bytes match");
    kani::cover!(
        !accepted && expected.is_empty(),
        "missing revision bytes never match"
    );
    kani::cover!(
        !accepted && expected.len() != current.len(),
        "different revision lengths never match"
    );
}

#[kani::proof]
#[kani::unwind(130)]
fn rendered_links_use_known_schemes() {
    let bytes: [u8; 128] = kani::any();
    let length: u8 = kani::any();
    kani::assume(usize::from(length) <= bytes.len());
    let length = usize::from(length);
    let bytes = &bytes[..length];
    let accepted = safe_external_link(bytes);
    if accepted {
        assert!(
            bytes.starts_with(b"https://")
                || bytes.starts_with(b"http://")
                || bytes.starts_with(b"mailto:")
        );
        let selected: u8 = kani::any();
        if usize::from(selected) < length {
            assert!(bytes[usize::from(selected)] > 32 && bytes[usize::from(selected)] != 127);
        }
    }
    kani::cover!(
        accepted && bytes.starts_with(b"https://"),
        "HTTPS destinations are allowed"
    );
    kani::cover!(
        accepted && bytes.starts_with(b"http://"),
        "HTTP destinations are allowed"
    );
    kani::cover!(
        accepted && bytes.starts_with(b"mailto:"),
        "email destinations are allowed"
    );
    kani::cover!(
        !accepted,
        "unsupported or malformed destinations are refused"
    );
}

#[kani::proof]
#[kani::unwind(66)]
fn html_attributes_cannot_end_their_quotes() {
    let bytes: [u8; 64] = kani::any();
    let length: u8 = kani::any();
    kani::assume(usize::from(length) <= bytes.len());
    let length = usize::from(length);
    let mut written = 0;
    let mut position = 0;
    for segment in attribute_segments(&bytes[..length]) {
        assert!(position < length);
        let original = bytes[position];
        let expected_length = match original {
            b'"' => 6,
            b'&' | b'\'' => 5,
            b'<' | b'>' => 4,
            _ => 1,
        };
        assert_eq!(segment.len(), expected_length);
        let selected: u8 = kani::any();
        let selected = usize::from(selected);
        if selected < segment.len() {
            let expected = match original {
                b'&' => b"&amp;"[selected],
                b'"' => b"&quot;"[selected],
                b'\'' => b"&#39;"[selected],
                b'<' => b"&lt;"[selected],
                b'>' => b"&gt;"[selected],
                _ => original,
            };
            assert_eq!(segment[selected], expected);
            assert!(!matches!(segment[selected], b'"' | b'\'' | b'<' | b'>'));
        }
        position += 1;
        written += segment.len();
    }
    assert_eq!(position, length);
    assert!(written <= length * 6);
    kani::cover!(
        length > 0 && bytes[0] == b'"',
        "quote inputs reach the encoder"
    );
    kani::cover!(length == 0, "empty attributes reach the encoder");
    kani::cover!(
        length > 0 && bytes[0] >= 128,
        "non-ASCII inputs reach the encoder"
    );
}

#[kani::proof]
#[kani::unwind(8)]
fn attribute_encoding_preserves_unicode_and_exact_entities() {
    let character: char = kani::any();
    let mut input = [0; 4];
    let text = character.encode_utf8(&mut input);
    let mut output = [0; 6];
    let mut written = 0;
    for segment in attribute_segments(text.as_bytes()) {
        for byte in segment {
            assert!(written < output.len());
            output[written] = *byte;
            written += 1;
        }
    }
    let output = &output[..written];
    assert!(std::str::from_utf8(output).is_ok());
    let expected = match character {
        '&' => b"&amp;".as_slice(),
        '"' => b"&quot;".as_slice(),
        '\'' => b"&#39;".as_slice(),
        '<' => b"&lt;".as_slice(),
        '>' => b"&gt;".as_slice(),
        _ => text.as_bytes(),
    };
    assert_eq!(output, expected);
    kani::cover!(character == '"', "entities encode the exact escaped bytes");
    kani::cover!(
        !character.is_ascii(),
        "all Unicode scalar encodings remain valid"
    );
}

#[kani::proof]
#[kani::unwind(6)]
fn revision_preconditions_allow_only_one_competing_writer() {
    let revision: [u8; 4] = kani::any();
    let first: [u8; 4] = kani::any();
    let second: [u8; 4] = kani::any();
    let first_wins: bool = kani::any();
    kani::assume(first != revision && second != revision);
    let mut current = revision;
    let (leading, trailing) = if first_wins {
        (first, second)
    } else {
        (second, first)
    };
    assert!(revision_matches(&revision, &current));
    current = leading;
    assert!(!revision_matches(&revision, &current));
    assert!(revision_matches(&leading, &current));
    if revision_matches(&revision, &current) {
        current = trailing;
    }
    assert_eq!(current, leading);
    kani::cover!(first_wins, "the first writer can win");
    kani::cover!(!first_wins, "the second writer can win");
}

#[kani::proof]
#[kani::unwind(47)]
fn attachment_extensions_cannot_escape_the_originals_directory() {
    let mut bytes: [u8; 45] = kani::any();
    let length: usize = kani::any();
    kani::assume((34..=45).contains(&length));
    bytes[..32].fill(b'a');
    bytes[32] = b'.';
    let name = &bytes[..length];
    let accepted = valid_attachment(name);
    if accepted {
        assert!(name[33..].iter().all(u8::is_ascii_alphanumeric));
        assert_eq!(name.iter().filter(|byte| **byte == b'.').count(), 1);
    }
    kani::cover!(accepted, "portable extensions are accepted");
    kani::cover!(!accepted, "path syntax in extensions is refused");
}
