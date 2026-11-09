use serde::Serialize;
use std::ops::Range;

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind {
    Image,
    Audio,
    Video,
    Pdf,
    File,
}

pub fn image_type(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(("png", "image/png"))
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some(("jpg", "image/jpeg"))
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(("gif", "image/gif"))
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some(("webp", "image/webp"))
    } else {
        None
    }
}

pub fn content_type(name: &str, bytes: &[u8]) -> (MediaKind, &'static str) {
    if let Some((_, mime)) = image_type(bytes) {
        return (MediaKind::Image, mime);
    }
    let extension = name
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if bytes.starts_with(b"%PDF-") {
        (MediaKind::Pdf, "application/pdf")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE") {
        (MediaKind::Audio, "audio/wav")
    } else if bytes.starts_with(b"fLaC") {
        (MediaKind::Audio, "audio/flac")
    } else if bytes.starts_with(b"ID3")
        || (bytes.len() > 2 && bytes[0] == 0xff && bytes[1] & 0xe6 == 0xe2)
    {
        (MediaKind::Audio, "audio/mpeg")
    } else if bytes.starts_with(b"OggS") {
        if bytes.windows(6).take(128).any(|part| part == b"theora") {
            (MediaKind::Video, "video/ogg")
        } else {
            (MediaKind::Audio, "audio/ogg")
        }
    } else if bytes.starts_with(b"\x1a\x45\xdf\xa3")
        && matches!(extension.as_str(), "webm" | "weba")
    {
        if extension == "weba" {
            (MediaKind::Audio, "audio/webm")
        } else {
            (MediaKind::Video, "video/webm")
        }
    } else if bytes.get(4..8) == Some(b"ftyp") {
        if matches!(extension.as_str(), "m4a" | "m4b") {
            (MediaKind::Audio, "audio/mp4")
        } else {
            (MediaKind::Video, "video/mp4")
        }
    } else {
        (MediaKind::File, "application/octet-stream")
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ByteRange {
    Full,
    Partial(Range<usize>),
    Unsatisfiable,
}

pub fn byte_range(header: Option<&str>, length: usize) -> ByteRange {
    let Some((unit, raw)) = header.and_then(|value| value.split_once('=')) else {
        return ByteRange::Full;
    };
    if !unit.eq_ignore_ascii_case("bytes") {
        return ByteRange::Full;
    }
    if raw.contains(',') || length == 0 {
        return ByteRange::Full;
    }
    let Some((start, end)) = raw.split_once('-') else {
        return ByteRange::Full;
    };
    let digits = |value: &str| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit());
    if start.is_empty() {
        if !digits(end) {
            return ByteRange::Full;
        }
        let count = end.parse::<usize>().unwrap_or(usize::MAX);
        if count == 0 {
            return ByteRange::Unsatisfiable;
        }
        return ByteRange::Partial(length.saturating_sub(count)..length);
    }
    if !digits(start) || (!end.is_empty() && !digits(end)) {
        return ByteRange::Full;
    }
    let first = start.parse::<usize>().unwrap_or(usize::MAX);
    let last = if end.is_empty() {
        usize::MAX
    } else {
        end.parse::<usize>().unwrap_or(usize::MAX)
    };
    if first >= length {
        return ByteRange::Unsatisfiable;
    }
    if last < first {
        return ByteRange::Full;
    }
    ByteRange::Partial(first..last.saturating_add(1).min(length))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_and_overflowing_ranges_keep_their_boundaries() {
        assert_eq!(byte_range(Some("bytes=2-"), 10), ByteRange::Partial(2..10));
        assert_eq!(byte_range(Some("BYTES=2-"), 10), ByteRange::Partial(2..10));
        assert_eq!(
            byte_range(Some("bytes=-999999999999999999999999999"), 10),
            ByteRange::Partial(0..10)
        );
        assert_eq!(
            byte_range(Some("bytes=999999999999999999999999999-"), 10),
            ByteRange::Unsatisfiable
        );
        for value in [
            "bytes=2-1",
            "bytes=a-2",
            "bytes=+2-4",
            "bytes=0-1,3-4",
            "other=0-2",
        ] {
            assert_eq!(byte_range(Some(value), 10), ByteRange::Full);
        }
    }
}
