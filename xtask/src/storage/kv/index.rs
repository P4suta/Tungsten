use super::Meta;
use crate::storage::database::Write;
use anyhow::Result;
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashSet};

const ALPHABET: u64 = 0x110000;

fn packed(characters: &[char]) -> u64 {
    characters.iter().fold(1, |value, character| {
        value * ALPHABET + u64::from(*character as u32)
    })
}

fn grams(text: &str) -> BTreeSet<u64> {
    let mut keys = HashSet::new();
    let mut window = ['\0'; 3];
    let mut length = 0;
    for character in text.chars() {
        window.rotate_left(1);
        window[2] = character;
        length = (length + 1).min(3);
        for width in 1..=length {
            keys.insert(packed(&window[3 - width..]));
        }
    }
    keys.into_iter().collect()
}

pub(super) fn count_key(prefix: &[u8]) -> Vec<u8> {
    [b"\0posting-count/".as_slice(), prefix].concat()
}

fn counted_prefix(key: &[u8]) -> Option<&[u8]> {
    match key.first()? {
        b'g' | b't' | b'P' | b'K' | b'@' | b'/' => key.get(..key.len().checked_sub(16)?),
        _ => None,
    }
}

pub(super) fn prefix(scope: u8, term: &str) -> Vec<u8> {
    let characters = term.chars().take(3).collect::<Vec<_>>();
    let mut prefix = vec![scope];
    prefix.extend_from_slice(&packed(&characters).to_be_bytes());
    prefix
}

pub(super) fn candidates(scope: u8, term: &str) -> Vec<Vec<u8>> {
    let characters = term.chars().collect::<Vec<_>>();
    if characters.len() <= 3 {
        return vec![prefix(scope, term)];
    }
    characters
        .windows(3)
        .map(|window| {
            let mut key = vec![scope];
            key.extend_from_slice(&packed(window).to_be_bytes());
            key
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(super) fn exact(title: &str) -> Vec<u8> {
    let mut prefix = vec![b'e'];
    prefix.extend_from_slice(&Sha256::digest(title.as_bytes()));
    prefix
}

pub(super) fn filter(scope: u8, value: &str) -> Vec<u8> {
    let mut prefix = vec![scope];
    prefix.extend_from_slice(&Sha256::digest(value.as_bytes()));
    prefix
}

pub(super) fn path(value: &str) -> Vec<u8> {
    let mut prefix = vec![b'/'];
    prefix.extend_from_slice(value.as_bytes());
    prefix.push(0);
    prefix
}

fn ordering(meta: &Meta) -> Vec<u8> {
    let mut key = vec![u8::from(!meta.pinned)];
    key.extend_from_slice(&(u128::MAX - meta.modified).to_be_bytes());
    key.extend(meta.name.as_bytes().iter().map(|byte| !byte));
    key.push(u8::MAX);
    key
}

fn entries(meta: &Meta, content: &str) -> BTreeSet<Vec<u8>> {
    let mut keys = BTreeSet::new();
    let descending = (u128::MAX - meta.id).to_be_bytes();
    for (scope, content) in [
        (b'g', format!("{}\n{content}", meta.name).to_lowercase()),
        (b't', meta.title.to_lowercase()),
    ] {
        for gram in grams(&content) {
            let mut key = vec![scope];
            key.extend_from_slice(&gram.to_be_bytes());
            key.extend_from_slice(&descending);
            keys.insert(key);
        }
    }
    let mut title = exact(&meta.title.to_lowercase());
    title.extend_from_slice(&descending);
    keys.insert(title);
    let mut numbered = vec![b'n'];
    numbered.extend_from_slice(&descending);
    keys.insert(numbered);
    let order = ordering(meta);
    let mut sorted = vec![b'o'];
    sorted.extend_from_slice(&order);
    keys.insert(sorted);
    for (scope, enabled) in [(b'p', meta.pinned), (b'k', meta.open_task)] {
        if enabled {
            let mut key = vec![scope];
            key.extend_from_slice(&order);
            keys.insert(key);
            let mut indexed = vec![scope.to_ascii_uppercase()];
            indexed.extend_from_slice(&descending);
            keys.insert(indexed);
        }
    }
    for tag in &meta.tags {
        let mut key = filter(b'#', tag);
        key.extend_from_slice(&order);
        keys.insert(key);
        let mut indexed = filter(b'@', tag);
        indexed.extend_from_slice(&descending);
        keys.insert(indexed);
    }
    let name = meta.name.to_ascii_lowercase();
    for (offset, character) in name.char_indices() {
        let mut indexed = path(&name[..offset + character.len_utf8()]);
        indexed.extend_from_slice(&descending);
        keys.insert(indexed);
    }
    for target in &meta.links {
        keys.insert(format!("l/{target}\0{:020}/{}", meta.id, meta.name).into_bytes());
    }
    for target in &meta.files {
        keys.insert(format!("a/{target}\0{}", meta.name).into_bytes());
    }
    keys
}

pub(super) struct Plan {
    before: BTreeSet<Vec<u8>>,
    after: BTreeSet<Vec<u8>>,
}

impl Plan {
    pub fn between(before: Option<(&Meta, &str)>, after: Option<(&Meta, &str)>) -> Self {
        Self {
            before: before.map_or_else(BTreeSet::new, |(meta, content)| entries(meta, content)),
            after: after.map_or_else(BTreeSet::new, |(meta, content)| entries(meta, content)),
        }
    }
    pub fn apply(&self, write: &mut dyn Write, name: &str) -> Result<()> {
        for key in self.before.difference(&self.after) {
            write.remove(key)?;
            if let Some(prefix) = counted_prefix(key) {
                write.adjust_count(&count_key(prefix), -1)?;
            }
        }
        for key in self.after.difference(&self.before) {
            write.put(key, name.as_bytes())?;
            if let Some(prefix) = counted_prefix(key) {
                write.adjust_count(&count_key(prefix), 1)?;
            }
        }
        Ok(())
    }
    pub fn changes(&self) -> usize {
        self.before.symmetric_difference(&self.after).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posting_counts_follow_durable_batches_edits_deletions_and_rollback() {
        use crate::storage::{Engine, database::Database};
        use tungsten::{
            domain::{NoteName, Revision},
            store::Note,
        };
        for engine in [Engine::SqliteKv, Engine::Redb, Engine::Fjall] {
            let directory = tempfile::tempdir().unwrap();
            let database = Database::open(engine, directory.path()).unwrap();
            let note = Note {
                name: NoteName::parse("One.md".into()).unwrap(),
                body: "# Title\n\nfirst first".into(),
                revision: Revision::of(b"initial"),
                modified: 42,
            };
            let other = Note {
                name: NoteName::parse("Two.md".into()).unwrap(),
                body: note.body.clone(),
                revision: note.revision.clone(),
                modified: note.modified,
            };
            let first = super::super::metadata(&note, 1, false);
            let second = super::super::metadata(&other, 2, false);
            let key = [
                b"\0posting-count/".as_slice(),
                prefix(b'g', "first").as_slice(),
            ]
            .concat();
            database
                .transaction(|write| {
                    Plan::between(None, Some((&first, &note.body))).apply(write, &first.name)?;
                    Plan::between(None, Some((&second, &other.body))).apply(write, &second.name)
                })
                .unwrap();
            assert_eq!(
                database.get(&key).unwrap(),
                Some(2u64.to_be_bytes().to_vec()),
                "{engine:?}"
            );
            assert!(
                database
                    .transaction::<()>(|write| {
                        Plan::between(Some((&first, &note.body)), None)
                            .apply(write, &first.name)?;
                        anyhow::bail!("injected abort")
                    })
                    .is_err()
            );
            assert_eq!(
                database.get(&key).unwrap(),
                Some(2u64.to_be_bytes().to_vec())
            );
            let changed = Note {
                name: note.name.clone(),
                body: "# Title\n\nsecond".into(),
                revision: Revision::of(b"changed"),
                modified: note.modified,
            };
            let updated = super::super::metadata(&changed, 1, false);
            database
                .transaction(|write| {
                    Plan::between(Some((&first, &note.body)), Some((&updated, &changed.body)))
                        .apply(write, &first.name)
                })
                .unwrap();
            assert_eq!(
                database.get(&key).unwrap(),
                Some(1u64.to_be_bytes().to_vec())
            );
            drop(database);
            let database = Database::open(engine, directory.path()).unwrap();
            assert_eq!(
                database.get(&key).unwrap(),
                Some(1u64.to_be_bytes().to_vec())
            );
            database
                .transaction(|write| {
                    Plan::between(Some((&second, &other.body)), None).apply(write, &second.name)
                })
                .unwrap();
            assert_eq!(database.get(&key).unwrap(), None);
        }
    }

    #[test]
    fn packed_grams_match_independent_unicode_windows() {
        for text in [
            "",
            "\0",
            "a",
            "aa",
            "ababa",
            "日本語😀 日本語",
            "\u{10ffff}\u{10000}é\n",
        ] {
            let characters = text.chars().collect::<Vec<_>>();
            let expected = (1..=3)
                .flat_map(|width| characters.windows(width).map(packed))
                .collect::<BTreeSet<_>>();
            assert_eq!(grams(text), expected, "{text:?}");
        }
    }

    #[test]
    fn packed_widths_and_unicode_boundaries_never_alias() {
        let alphabet = ['\0', 'a', '日', '😀', '\u{10ffff}'];
        let mut keys = BTreeSet::new();
        for first in alphabet {
            assert!(keys.insert(packed(&[first])));
            for second in alphabet {
                assert!(keys.insert(packed(&[first, second])));
                for third in alphabet {
                    assert!(keys.insert(packed(&[first, second, third])));
                }
            }
        }
        assert_eq!(keys.len(), 155);
    }

    #[test]
    fn repeated_text_edits_preserve_unchanged_postings() {
        use tungsten::{
            domain::{NoteName, Revision},
            store::Note,
        };
        let before = Note {
            name: NoteName::parse("One.md".into()).unwrap(),
            body: "# Title\n\nalpha beta alpha beta ".into(),
            revision: Revision::of(b"before"),
            modified: 42,
        };
        let mut after = Note {
            name: before.name.clone(),
            body: before.body.clone(),
            revision: before.revision.clone(),
            modified: before.modified,
        };
        after.body.push_str("alpha beta ");
        let first = super::super::metadata(&before, 1, false);
        let second = super::super::metadata(&after, 1, false);
        let plan = Plan::between(Some((&first, &before.body)), Some((&second, &after.body)));
        assert_eq!(plan.changes(), 0);
    }
}
