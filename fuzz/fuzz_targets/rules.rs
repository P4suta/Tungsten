#![no_main]

#[path = "../../src/rules.rs"]
mod rules;

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if rules::valid_name(data) {
        assert!(data.len() <= 240);
        assert!(data.ends_with(b".md"));
        assert_ne!(data[0], b'.');
        assert!(!data.split(|byte| *byte == b'/').any(|part| part == b".."));
        assert!(
            !data
                .iter()
                .any(|byte| matches!(*byte, 0..=31 | 127 | b'\\'))
        );
    }
    if rules::valid_attachment(data) || rules::valid_id(data) {
        assert!(!data.contains(&b'/') && !data.contains(&b'\\'));
    }
    if rules::safe_external_link(data) {
        assert!(!data.iter().any(|byte| *byte <= 32 || *byte == 127));
    }
    assert_eq!(rules::revision_matches(data, data), !data.is_empty());
    if let Ok(text) = std::str::from_utf8(data) {
        assert!(
            !rules::escape_attribute(text)
                .bytes()
                .any(|byte| matches!(byte, b'"' | b'\'' | b'<' | b'>'))
        );
    }
});
