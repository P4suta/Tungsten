pub fn valid_name(bytes: &[u8]) -> bool {
    if !(4..=240).contains(&bytes.len())
        || !bytes.ends_with(b".md")
        || bytes[bytes.len() - 4] == b' '
    {
        return false;
    }
    let mut components = 1u8;
    let mut start = true;
    let mut previous = 0;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte < 32 || matches!(byte, 127 | b'\\') {
            return false;
        }
        if byte == b'/' {
            if start || previous == b' ' || components == 16 {
                return false;
            }
            components += 1;
            start = true;
        } else {
            if start && matches!(byte, b'.' | b' ') {
                return false;
            }
            start = false;
        }
        previous = byte;
        index = index.saturating_add(1);
    }
    true
}

pub fn valid_id(bytes: &[u8]) -> bool {
    if bytes.len() != 32 {
        return false;
    }
    for &byte in bytes {
        if !byte.is_ascii_digit() && !(b'a'..=b'f').contains(&byte) {
            return false;
        }
    }
    true
}

pub fn valid_attachment(bytes: &[u8]) -> bool {
    if !(34..=45).contains(&bytes.len()) || !valid_id(&bytes[..32]) || bytes[32] != b'.' {
        return false;
    }
    for byte in &bytes[33..] {
        if !byte.is_ascii_alphanumeric() {
            return false;
        }
    }
    true
}

pub fn safe_external_link(bytes: &[u8]) -> bool {
    if !(bytes.starts_with(b"https://")
        || bytes.starts_with(b"http://")
        || bytes.starts_with(b"mailto:"))
    {
        return false;
    }
    for &byte in bytes {
        if byte <= 32 || byte == 127 {
            return false;
        }
    }
    true
}

pub fn revision_matches(expected: &[u8], current: &[u8]) -> bool {
    !expected.is_empty() && expected == current
}

pub fn escape_attribute(text: &str) -> String {
    let mut escaped = Vec::with_capacity(text.len());
    for segment in attribute_segments(text.as_bytes()) {
        escaped.extend_from_slice(segment);
    }
    String::from_utf8(escaped).expect("Escaping must preserve UTF-8.")
}

fn attribute_segments(input: &[u8]) -> impl Iterator<Item = &[u8]> {
    input.chunks(1).map(|byte| match byte[0] {
        b'&' => b"&amp;".as_slice(),
        b'"' => b"&quot;",
        b'\'' => b"&#39;",
        b'<' => b"&lt;",
        b'>' => b"&gt;",
        _ => byte,
    })
}

#[cfg(kani)]
#[path = "rules/proofs.rs"]
mod proofs;
