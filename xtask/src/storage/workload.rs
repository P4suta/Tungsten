use super::{Case, Content, Engine, Options, adapter::Storage, runner::Samples};
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::{
    io::{self, Read},
    path::Path,
    process::Command,
};
use tungsten::domain::Revision;

pub(super) struct GeneratedFile {
    offset: usize,
    length: usize,
    seed: u64,
    compressible: bool,
    digest: Sha256,
}

impl GeneratedFile {
    pub fn new(length: usize, seed: u64, compressible: bool) -> Self {
        Self {
            offset: 0,
            length,
            seed,
            compressible,
            digest: Sha256::new(),
        }
    }
    fn byte(&self, index: usize) -> u8 {
        if self.compressible {
            b'A'
        } else {
            let mut value = (index as u64)
                .wrapping_add(self.seed)
                .wrapping_add(0x9e3779b97f4a7c15);
            value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
            value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
            (value ^ (value >> 31)) as u8
        }
    }
    fn revision(self) -> Result<Revision> {
        Ok(Revision::from_digest(self.digest))
    }
}

impl Read for GeneratedFile {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let count = output.len().min(self.length - self.offset);
        for (index, byte) in output[..count].iter_mut().enumerate() {
            *byte = self.byte(self.offset + index);
        }
        self.offset += count;
        self.digest.update(&output[..count]);
        Ok(count)
    }
}

fn note(index: u32, options: &Options) -> (String, String) {
    let name = format!("Note-{index:08}.md");
    let target = if options.case == Case::Hub {
        0
    } else {
        index.saturating_sub(1)
    };
    let mut body = format!(
        "# Topic {index}\n\nNeedle-{index:08} 日本語 #research\n\n- [ ] Read\n\n[reference](Note-{target:08}.md)\n\n"
    );
    let phrases = [
        "A connected thought. ",
        "A reading observation. ",
        "A research question. ",
        "A quoted fragment. ",
    ];
    let phrase = if options.case == Case::Unicode {
        "かな カナ e\u{301} É Σ İ 🧑\u{200d}💻。 "
    } else {
        phrases[(u64::from(index).wrapping_add(options.seed) % phrases.len() as u64) as usize]
    };
    if options.case == Case::Unicode {
        body.push_str(phrase);
    }
    fill(
        &mut body,
        options.note_bytes as usize,
        options.seed.wrapping_add(u64::from(index)),
        options.content,
        phrase,
    );
    let mut end = (options.note_bytes as usize).min(body.len());
    while !body.is_char_boundary(end) {
        end -= 1;
    }
    body.truncate(end);
    (name, body)
}

fn fill(body: &mut String, target: usize, mut state: u64, content: Content, repeated: &str) {
    const WORDS: [&str; 32] = [
        "analysis",
        "archive",
        "argument",
        "attention",
        "boundary",
        "chapter",
        "choice",
        "context",
        "culture",
        "design",
        "detail",
        "discovery",
        "drawing",
        "evidence",
        "example",
        "experiment",
        "history",
        "identity",
        "language",
        "learning",
        "meaning",
        "method",
        "observation",
        "pattern",
        "practice",
        "question",
        "reading",
        "relation",
        "research",
        "structure",
        "thought",
        "writing",
    ];
    while body.len() < target {
        if content == Content::Repeated {
            body.push_str(repeated);
        } else {
            state = state.wrapping_add(0x9e3779b97f4a7c15);
            let mut value = state;
            value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
            value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
            let choice = value ^ (value >> 31);
            if content == Content::Alphanumeric {
                const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
                body.push(char::from(
                    ALPHABET[(choice % ALPHABET.len() as u64) as usize],
                ));
            } else {
                body.push_str(WORDS[(choice % WORDS.len() as u64) as usize]);
                body.push(' ');
            }
        }
    }
}

fn expected_query(options: &Options, query: &str, limit: usize) -> Vec<String> {
    let query = query.trim().to_lowercase();
    let terms: Vec<_> = query.split_whitespace().collect();
    (0..options.count)
        .rev()
        .filter_map(|index| {
            let (name, body) = note(index, options);
            let content = format!("{name}\n{body}").to_lowercase();
            let metadata = tungsten::markdown::index_metadata(&name, &body);
            let matches = terms.iter().all(|term| {
                let (term, negate) = term
                    .strip_prefix('-')
                    .map_or((*term, false), |value| (value, true));
                let value = if let Some(tag) =
                    term.strip_prefix('#').or_else(|| term.strip_prefix("tag:"))
                {
                    metadata.tags.iter().any(|value| value == tag)
                } else if let Some(path) = term.strip_prefix("path:") {
                    name.to_ascii_lowercase().starts_with(path)
                } else if term == "task:open" {
                    body.contains("[ ]")
                } else {
                    content.contains(term)
                };
                value != negate
            });
            matches.then_some(name)
        })
        .take(limit)
        .collect()
}

fn files(
    options: &Options,
    store: &mut Storage,
    samples: &mut Samples,
) -> Result<Vec<(String, usize, u64, bool)>> {
    let count = if options.case == Case::SmallFiles {
        options.count
    } else {
        3
    };
    let mut ids = Vec::new();
    for index in 0..count {
        let length = if options.case == Case::LargeFiles {
            options.file_bytes.max(16 * 1024 * 1024)
        } else {
            options.file_bytes
        } as usize;
        let compressible = options.case != Case::IncompressibleFiles && index % 2 == 0;
        let seed = options.seed.wrapping_add(u64::from(index));
        let mut input = GeneratedFile::new(length, seed, compressible);
        let id = samples.measure("upload", length as u64, || {
            store.upload("original.bin", length, &mut input)
        })?;
        let expected_revision = input.revision()?;
        samples.input(expected_revision.as_str().as_bytes());
        let mut digest = Sha256::new();
        for start in (0..length).step_by(64 * 1024) {
            let end = (start + 64 * 1024).min(length);
            let bytes = samples.measure("download-range", (end - start) as u64, || {
                store.range(&id, start..end)
            })?;
            ensure!(bytes.len() == end - start, "short attachment response");
            digest.update(&bytes);
        }
        ensure!(
            Revision::from_digest(digest) == expected_revision,
            "original attachment changed"
        );
        ids.push((id, length, seed, compressible));
    }
    Ok(ids)
}

pub(super) fn execute(
    options: &Options,
    engine: Engine,
    root: &Path,
    samples: &mut Samples,
) -> Result<()> {
    let path = root.join("database");
    let mut store = samples.measure("open", 0, || Storage::open(engine, &path))?;
    if options.load_fixture {
        let start = std::time::Instant::now();
        let mut bytes = 0u64;
        let count = store.load_fixture((0..options.count).map(|index| {
            let (name, body) = note(index, options);
            samples.input(name.as_bytes());
            samples.input(body.as_bytes());
            bytes += body.len() as u64;
            Ok(tungsten::store::Note {
                name: tungsten::domain::NoteName::parse(name)?,
                revision: Revision::of(body.as_bytes()),
                body,
                modified: 1767225600000 + u128::from(index),
            })
        }))?;
        ensure!(
            count == options.count as usize,
            "incomplete fixture loading"
        );
        samples.observe(
            "fixture-load",
            bytes,
            start.elapsed().as_micros().try_into()?,
        )?;
        let audit = std::time::Instant::now();
        for index in 0..options.count {
            let (name, body) = note(index, options);
            let saved = store.read(&name)?;
            ensure!(
                saved.body == body && saved.revision == Revision::of(body.as_bytes()),
                "fixture audit failed for {name}"
            );
        }
        samples.observe(
            "fixture-audit",
            bytes,
            audit.elapsed().as_micros().try_into()?,
        )?;
    } else {
        for index in 0..options.count {
            let (name, body) = note(index, options);
            samples.input(name.as_bytes());
            samples.input(body.as_bytes());
            samples.measure("create-note", body.len() as u64, || {
                let revision = store.put(&name, &body, None)?;
                ensure!(
                    revision == Revision::of(body.as_bytes()),
                    "incorrect saved revision"
                );
                Ok(())
            })?;
            if options.case == Case::ShortNotes && index % 8 == 0 {
                samples.measure("interleaved-read", body.len() as u64, || {
                    ensure!(
                        store.read(&name)?.body == body,
                        "interleaved reading lost a note"
                    );
                    Ok(())
                })?;
            }
            if index % 256 == 0 {
                super::runner::check_allocation(root, options.allocation_bytes())?;
            }
        }
    }
    for index in (0..options.count).step_by((options.count / 64).max(1) as usize) {
        let (name, body) = note(index, options);
        samples.measure("read-note", body.len() as u64, || {
            let note = store.read(&name)?;
            ensure!(
                note.body == body && note.revision == Revision::of(body.as_bytes()),
                "stored Markdown changed"
            );
            Ok(())
        })?;
    }
    let mut queries = vec![
        "日".to_owned(),
        "日本".into(),
        "日本語".into(),
        format!("needle-{:08}", options.count - 1),
        "absent-token".into(),
        "-absent-token 日本".into(),
        "path:note-0000 日本".into(),
        "#research 日本".into(),
        "tag:research 日本".into(),
        "task:open 日本".into(),
    ];
    if options.case == Case::Unicode {
        queries.extend(
            ["é", "σ", "i\u{307}", "e\u{301}", "🧑\u{200d}💻"]
                .into_iter()
                .map(str::to_owned),
        );
    }
    for query in queries {
        let expected = expected_query(options, &query, 120);
        for _ in 0..20 {
            samples.input(query.as_bytes());
            samples.measure("search", 0, || {
                ensure!(
                    store.search(&query, false, 120)? == expected,
                    "incorrect search results for {query}"
                );
                Ok(())
            })?;
        }
    }
    if options.case == Case::SearchSelectivity {
        for (phase, query) in [
            ("search-common-positive", "needle"),
            ("search-common-negative", "-needle"),
            ("search-window-positive", "0000000000000000"),
            ("search-window-negative", "-0000000000000000"),
            ("search-filtered-negative", "path:note-00000000 -needle"),
        ] {
            let expected = expected_query(options, query, 120);
            for _ in 0..20 {
                samples.input(query.as_bytes());
                samples.measure(phase, 0, || {
                    ensure!(
                        store.search(query, false, 120)? == expected,
                        "incorrect search results for {query}"
                    );
                    Ok(())
                })?;
            }
        }
    }
    if options.case == Case::Hub && options.note_bytes >= 128 {
        let incoming =
            samples.measure("incoming-links", 0, || store.incoming("Note-00000000.md"))?;
        ensure!(
            incoming.len() == (options.count as usize).min(120),
            "incorrect hub neighborhood"
        );
    }
    if options.load_fixture {
        for index in 0..128 {
            let (_, body) = note(options.count + index, options);
            let name = format!("Probe-{index:08}.md");
            samples.input(name.as_bytes());
            samples.input(body.as_bytes());
            let revision = samples.measure("create-note", body.len() as u64, || {
                store.put(&name, &body, None)
            })?;
            ensure!(
                revision == Revision::of(body.as_bytes()) && store.read(&name)?.body == body,
                "fixture probe creation lost content"
            );
        }
    }
    if matches!(
        options.case,
        Case::Regression | Case::LongEdits | Case::Churn | Case::Recovery
    ) {
        let name = "Hot.md";
        let mut body = "# Long-lived thought\n\n".to_owned();
        let target = if options.case == Case::LongEdits {
            options.note_bytes.max(256 * 1024) as usize
        } else {
            options.note_bytes as usize
        };
        fill(
            &mut body,
            target.saturating_sub(128),
            options.seed,
            options.content,
            "A paragraph that grows while typing. ",
        );
        let mut revision = store.put(name, &body, None)?;
        for index in 0..options.updates {
            body.push_str(&format!("\nThought {index}."));
            if body.len() > tungsten::store::MAX_NOTE_BYTES {
                body.truncate(target.saturating_sub(1024));
            }
            samples.input(body.as_bytes());
            let prior = revision.clone();
            revision = samples.measure("save-growing-note", body.len() as u64, || {
                store.put(name, &body, Some(&prior))
            })?;
            if let Some(timings) = store.take_timings() {
                samples.observe("prepare-update", body.len() as u64, timings.preparation_us)?;
                samples.observe("commit-update", body.len() as u64, timings.transaction_us)?;
                samples.index_changes(timings.index_changes)?;
            }
            ensure!(
                store
                    .put(name, "stale", Some(&Revision::of(b"stale")))
                    .is_err(),
                "stale writing overwrote a note"
            );
            ensure!(store.read(name)?.body == body, "a save lost text");
        }
        ensure!(
            store.history_count(name)? > 0,
            "missing recovery checkpoint"
        );
    }
    let mut ids = if matches!(
        options.case,
        Case::Regression
            | Case::SmallFiles
            | Case::LargeFiles
            | Case::IncompressibleFiles
            | Case::SharedFiles
            | Case::Recovery
            | Case::Churn
    ) {
        files(options, &mut store, samples)?
    } else {
        Vec::new()
    };
    let group = if matches!(
        options.case,
        Case::Regression | Case::Recovery | Case::Churn
    ) {
        let group =
            samples.measure("create-workspace", 0, || store.create_workspace("Research"))?;
        let (name, _) = note(0, options);
        let revision = store.read(&name)?.revision;
        samples.measure("pin-note", 0, || store.pin(&name, &revision))?;
        samples.measure("workspace-membership", 0, || {
            store.add_member(&group, &name)
        })?;
        ensure!(
            store.members(&group)? == vec![name],
            "workspace membership was lost"
        );
        Some(group)
    } else {
        None
    };
    if options.case == Case::Churn {
        for index in 0..options.updates {
            let name = format!("Churn-{index:08}.md");
            let revision = store.put(&name, "# Before deletion", None)?;
            samples.measure("delete-note", 0, || store.delete(&name, &revision))?;
            ensure!(store.read(&name).is_err(), "deleted note remained live");
            samples.measure("recreate-note", 0, || {
                store.put(&name, "# Independent replacement", None)
            })?;
        }
    }
    if options.case == Case::Recovery && !ids.is_empty() {
        let references = ids
            .iter()
            .map(|(id, ..)| format!("[original](/.attachments/{id})"))
            .collect::<Vec<_>>()
            .join("\n");
        let revision = store.put(
            "Retained.md",
            &format!("# Recovery references\n\n{references}"),
            None,
        )?;
        store.delete("Retained.md", &revision)?;
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis();
        let week = 7 * 24 * 60 * 60 * 1000;
        samples.measure("retained-file-cleanup", 0, || {
            store.cleanup(timestamp + week / 2)
        })?;
        for (id, length, ..) in &ids {
            store.range(id, 0..(*length).min(16))?;
        }
        samples.measure("expired-recovery-cleanup", 0, || {
            store.cleanup(timestamp + week + 1000)
        })?;
        for (id, length, ..) in &ids {
            store.range(id, 0..(*length).min(16))?;
        }
        samples.measure("unreferenced-file-cleanup", 0, || {
            store.cleanup(timestamp + 2 * week + 2000)
        })?;
        for (id, length, ..) in &ids {
            ensure!(
                store.range(id, 0..(*length).min(16)).is_err(),
                "expired original remained available"
            );
        }
        ids.clear();
    }
    if options.case == Case::SharedFiles
        && let Some((id, ..)) = ids.first()
    {
        for index in 0..options.count {
            let body = format!("# Shared reference {index}\n\n[original](/.attachments/{id})");
            samples.input(format!("shared-reference-{index}").as_bytes());
            samples.measure("shared-reference", body.len() as u64, || {
                store.put(&format!("Shared-{index:08}.md"), &body, None)
            })?;
        }
    }
    samples.measure("rollback", 0, || store.verify_rollback())?;
    if matches!(options.case, Case::Regression | Case::Capacity) {
        let mut short = io::Cursor::new([42u8; 16]);
        ensure!(
            store.upload("short.bin", 1024, &mut short).is_err(),
            "a short reader committed a file"
        );
    }
    store = samples.measure("reopen", 0, || store.reopen())?;
    let (name, body) = note(options.count - 1, options);
    ensure!(store.read(&name)?.body == body, "restart lost Markdown");
    if matches!(
        options.case,
        Case::Regression | Case::Recovery | Case::Churn
    ) {
        let archive = root.join("archive");
        samples.measure("complete-backup", 0, || store.backup(&archive))?;
        let restored = root.join("restored");
        samples.measure("complete-restore", 0, || {
            Storage::restore(engine, &archive, &restored)
        })?;
        let recovered = Storage::open(engine, &restored)?;
        ensure!(
            recovered.read(&name)?.body == body,
            "restoration lost Markdown"
        );
        if let Some(group) = &group {
            let (first, _) = note(0, options);
            ensure!(
                recovered.members(group)? == vec![first.clone()],
                "restoration lost membership"
            );
            ensure!(
                recovered.search("is:pinned", false, 10)? == vec![first],
                "restoration lost a pin"
            );
        }
        for (id, length, seed, compressible) in &ids {
            let mut expected = GeneratedFile::new(*length, *seed, *compressible);
            for start in (0..*length).step_by(64 * 1024) {
                let mut bytes = vec![0; (*length - start).min(64 * 1024)];
                expected.read_exact(&mut bytes)?;
                ensure!(
                    recovered.range(id, start..start + bytes.len())? == bytes,
                    "restoration changed original bytes"
                );
            }
        }
        let plain = root.join("portable-sqlite");
        samples.measure("portable-restore", 0, || {
            Storage::restore(Engine::Sqlite, &archive, &plain)
        })?;
        ensure!(
            Storage::open(Engine::Sqlite, &plain)?.read(&name)?.body == body,
            "portable restoration lost Markdown"
        );
    }
    if matches!(options.case, Case::Regression | Case::Crash) {
        store.put("Crash.md", "# Before interrupted write", None)?;
        drop(store);
        for commit in [false, true] {
            let mut command = Command::new(std::env::current_exe()?);
            command
                .args(["storage-crash-native", "--engine", engine.name(), "--data"])
                .arg(&path);
            if commit {
                command.arg("--commit");
            }
            let status = command.status().context("crash fixture failed to start")?;
            ensure!(
                status.code() == Some(73),
                "crash fixture did not reach its injection point"
            );
            let reopened = samples.measure("crash-reopen", 0, || Storage::open(engine, &path))?;
            let expected = if commit {
                "# Durable acknowledgement"
            } else {
                "# Before interrupted write"
            };
            ensure!(
                reopened.read("Crash.md")?.body == expected,
                "interrupted commit violated durability"
            );
        }
    }
    super::runner::check_allocation(root, options.allocation_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alphanumeric_inputs_reproduce_many_distinct_search_grams() {
        let generate = |seed| {
            let mut body = String::new();
            fill(&mut body, 262144, seed, Content::Alphanumeric, "unused");
            body
        };
        let first = generate(42);
        assert_eq!(first, generate(42));
        assert_ne!(first, generate(43));
        assert!(
            first
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        );
        assert!(
            first
                .as_bytes()
                .windows(3)
                .collect::<std::collections::HashSet<_>>()
                .len()
                > 40000
        );
    }
}
