mod adapter;
mod allocation;
mod database;
mod kv;
mod runner;
mod workload;

use anyhow::{Result, ensure};
use clap::{Args, ValueEnum};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Engine {
    Sqlite,
    SqliteKv,
    Redb,
    Fjall,
    All,
}

impl Engine {
    pub fn name(self) -> &'static str {
        match self {
            Self::Sqlite => "sqlite",
            Self::SqliteKv => "sqlite-kv",
            Self::Redb => "redb",
            Self::Fjall => "fjall",
            Self::All => "all",
        }
    }
    fn selected(self) -> Vec<Self> {
        if self == Self::All {
            vec![Self::Sqlite, Self::SqliteKv, Self::Redb, Self::Fjall]
        } else {
            vec![self]
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Content {
    #[default]
    Repeated,
    Varied,
    Alphanumeric,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Case {
    Regression,
    ShortNotes,
    LongEdits,
    Burst,
    Hub,
    Unicode,
    SearchSelectivity,
    SmallFiles,
    LargeFiles,
    IncompressibleFiles,
    SharedFiles,
    Churn,
    Recovery,
    Crash,
    Capacity,
    All,
}

impl Case {
    fn selected(self) -> Vec<Self> {
        if self == Self::All {
            vec![
                Self::Regression,
                Self::ShortNotes,
                Self::LongEdits,
                Self::Burst,
                Self::Hub,
                Self::Unicode,
                Self::SearchSelectivity,
                Self::SmallFiles,
                Self::LargeFiles,
                Self::IncompressibleFiles,
                Self::SharedFiles,
                Self::Churn,
                Self::Recovery,
                Self::Crash,
                Self::Capacity,
            ]
        } else {
            vec![self]
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Regression => "regression",
            Self::ShortNotes => "short-notes",
            Self::LongEdits => "long-edits",
            Self::Burst => "burst",
            Self::Hub => "hub",
            Self::Unicode => "unicode",
            Self::SearchSelectivity => "search-selectivity",
            Self::SmallFiles => "small-files",
            Self::LargeFiles => "large-files",
            Self::IncompressibleFiles => "incompressible-files",
            Self::SharedFiles => "shared-files",
            Self::Churn => "churn",
            Self::Recovery => "recovery",
            Self::Crash => "crash",
            Self::Capacity => "capacity",
            Self::All => "all",
        }
    }
}

#[derive(Clone, Debug, Args, Serialize, Deserialize)]
pub(crate) struct Options {
    #[arg(long, value_enum, default_value = "sqlite")]
    engine: Engine,
    #[arg(long, value_enum, default_value = "regression")]
    case: Case,
    #[arg(long, default_value_t = 128, value_parser = clap::value_parser!(u32).range(1..=1_000_000))]
    count: u32,
    #[arg(long, default_value_t = 1024, value_parser = clap::value_parser!(u32).range(1..=1_048_576))]
    note_bytes: u32,
    #[arg(long, default_value_t = 262144, value_parser = clap::value_parser!(u32).range(0..=67_108_864))]
    file_bytes: u32,
    #[arg(long, default_value_t = 16, value_parser = clap::value_parser!(u32).range(1..=1000))]
    updates: u32,
    #[arg(long, default_value_t = 42)]
    seed: u64,
    #[arg(long, value_enum, default_value = "repeated")]
    content: Content,
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..=9))]
    repeats: u32,
    #[arg(skip)]
    replicate: u32,
    #[arg(long, default_value_t = 8, value_parser = clap::value_parser!(u64).range(1..=8))]
    allocation_gib: u64,
    #[arg(long, conflicts_with = "allocation_gib", value_parser = clap::value_parser!(u64).range(16..=8192))]
    allocation_mib: Option<u64>,
    #[arg(long, default_value_t = 3600, value_parser = clap::value_parser!(u64).range(900..=7200))]
    max_seconds: u64,
    #[arg(long)]
    load_fixture: bool,
    #[arg(long)]
    baseline: Option<PathBuf>,
    #[arg(long)]
    output: Option<PathBuf>,
    #[arg(long)]
    dry_run: bool,
}

impl Options {
    fn allocation_bytes(&self) -> u64 {
        self.allocation_mib
            .map_or(self.allocation_gib * 1024 * 1024 * 1024, |mib| {
                mib * 1024 * 1024
            })
    }

    fn experiments(&self) -> Vec<Self> {
        (0..self.repeats)
            .flat_map(|replicate| {
                let mut engines = self.engine.selected();
                let length = engines.len();
                engines.rotate_left(replicate as usize % length);
                engines.into_iter().flat_map(move |engine| {
                    self.case.selected().into_iter().map(move |case| {
                        let mut options = self.clone();
                        options.engine = engine;
                        options.case = case;
                        options.replicate = replicate + 1;
                        options
                    })
                })
            })
            .collect()
    }
    fn validate(&self) -> Result<()> {
        ensure!((1..=1_000_000).contains(&self.count), "invalid note count");
        ensure!(
            (1..=1_048_576).contains(&self.note_bytes) && self.file_bytes <= 67_108_864,
            "payload limit exceeded"
        );
        ensure!((1..=1000).contains(&self.updates), "invalid update count");
        ensure!((1..=9).contains(&self.repeats), "invalid repetition count");
        ensure!(
            (1..=8).contains(&self.allocation_gib)
                && self
                    .allocation_mib
                    .is_none_or(|mib| (16..=8192).contains(&mib))
                && (900..=7200).contains(&self.max_seconds),
            "invalid campaign limits"
        );
        ensure!(
            self.case != Case::All || self.baseline.is_none(),
            "one baseline requires one workload case"
        );
        for case in self.case.selected() {
            let notes = u64::from(self.count) * u64::from(self.note_bytes) * 6;
            let file_bytes = if case == Case::LargeFiles {
                self.file_bytes.max(16 * 1024 * 1024)
            } else {
                self.file_bytes
            };
            let files = u64::from(file_bytes)
                * if case == Case::SmallFiles {
                    u64::from(self.count)
                } else {
                    3
                };
            ensure!(
                notes
                    .checked_add(files * 4)
                    .is_some_and(|bytes| bytes <= self.allocation_bytes()),
                "planned data exceeds the storage allocation"
            );
        }
        Ok(())
    }
}

pub(crate) fn run(root: &Path, options: Options) -> Result<()> {
    runner::run(root, options)
}
pub(crate) fn summarize(path: &Path, engine: Engine, repeats: u32) -> Result<()> {
    runner::summarize(path, engine, repeats)
}
pub(crate) fn native(request: &Path) -> Result<()> {
    runner::native(request)
}
pub(crate) fn crash(engine: Engine, data: &Path, commit: bool) -> Result<()> {
    adapter::crash(engine, data, commit)
}

pub(crate) fn release(root: &Path) -> Result<()> {
    run(
        root,
        Options {
            engine: Engine::Sqlite,
            case: Case::Regression,
            count: 64,
            note_bytes: 1024,
            file_bytes: 262144,
            updates: 16,
            seed: 42,
            content: Content::Repeated,
            repeats: 1,
            replicate: 0,
            allocation_gib: 8,
            allocation_mib: None,
            max_seconds: 3600,
            load_fixture: false,
            baseline: None,
            output: None,
            dry_run: false,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{Engine, adapter::Storage};
    use tungsten::domain::Revision;

    #[test]
    fn empty_search_filters_preserve_app_semantics_with_and_without_text() {
        for engine in Engine::All.selected() {
            let directory = tempfile::tempdir().unwrap();
            let mut store = Storage::open(engine, directory.path()).unwrap();
            store.put("One.md", "# One\n\n日本語", None).unwrap();
            for query in [
                "path:",
                "日本 path:",
                "path: 日本",
                "-",
                "日本 -",
                "日本 -tag:",
            ] {
                assert_eq!(
                    store.search(query, false, 10).unwrap(),
                    vec!["One.md"],
                    "{}: {query}",
                    engine.name()
                );
            }
            for query in ["-path:", "日本 -path:", "#", "日本 #", "tag:", "日本 tag:"] {
                assert!(
                    store.search(query, false, 10).unwrap().is_empty(),
                    "{}: {query}",
                    engine.name()
                );
            }
        }
    }

    #[test]
    fn every_engine_preserves_search_conflicts_ranges_and_portable_recovery() {
        for engine in Engine::All.selected() {
            let directory = tempfile::tempdir().unwrap();
            let data = directory.path().join("data");
            let archive = directory.path().join("archive");
            let restored = directory.path().join("restored");
            let mut store = Storage::open(engine, &data).unwrap();
            let first = store
                .put("One.md", "# Thought\n\n日本語 [reference](Two.md)", None)
                .unwrap();
            let group = store.create_workspace("Research").unwrap();
            store.add_member(&group, "One.md").unwrap();
            store.pin("One.md", &first).unwrap();
            store.put("Two.md", "# Other", None).unwrap();
            assert_eq!(store.search("日本", false, 10).unwrap(), vec!["One.md"]);
            assert_eq!(store.search("日", false, 10).unwrap(), vec!["One.md"]);
            assert_eq!(store.incoming("Two.md").unwrap(), vec!["One.md"]);
            assert!(
                store
                    .put("One.md", "stale", Some(&Revision::of(b"wrong")))
                    .is_err()
            );
            assert_eq!(store.read("One.md").unwrap().revision, first);
            let current = store
                .put(
                    "One.md",
                    "# Thought\n\n日本語 [reference](Two.md)\n\n## Added",
                    Some(&first),
                )
                .unwrap();
            assert_eq!(store.history_count("One.md").unwrap(), 1);
            assert_eq!(
                store.search("is:pinned", false, 10).unwrap(),
                vec!["One.md"]
            );
            let bytes: Vec<_> = (0..150_000).map(|value| (value % 251) as u8).collect();
            let id = store
                .upload("file.bin", bytes.len(), &mut bytes.as_slice())
                .unwrap();
            assert_eq!(store.range(&id, 65520..65590).unwrap(), bytes[65520..65590]);
            store.verify_rollback().unwrap();
            store.backup(&archive).unwrap();
            drop(store);
            Storage::restore(engine, &archive, &restored).unwrap();
            let store = Storage::open(engine, &restored).unwrap();
            assert_eq!(store.read("One.md").unwrap().revision, current);
            assert_eq!(store.search("日本", false, 10).unwrap(), vec!["One.md"]);
            assert_eq!(store.range(&id, 0..bytes.len()).unwrap(), bytes);
            assert_eq!(store.history_count("One.md").unwrap(), 1);
            assert_eq!(store.members(&group).unwrap(), vec!["One.md"]);
            assert_eq!(
                store.search("is:pinned", false, 10).unwrap(),
                vec!["One.md"]
            );
        }
    }

    #[test]
    fn every_engine_retains_referenced_files_until_recovery_and_grace_expire() {
        for engine in Engine::All.selected() {
            let directory = tempfile::tempdir().unwrap();
            let mut store = Storage::open(engine, directory.path()).unwrap();
            let id = store
                .upload("original.bin", 3, &mut b"raw".as_slice())
                .unwrap();
            let body = format!("# Attachment\n\n[original](/.attachments/{id})");
            let revision = store.put("Keep.md", &body, None).unwrap();
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis();
            let week = 7 * 24 * 60 * 60 * 1000;
            store.cleanup(timestamp + week + 1000).unwrap();
            assert_eq!(store.range(&id, 0..3).unwrap(), b"raw");
            store.delete("Keep.md", &revision).unwrap();
            assert!(store.read("Keep.md").is_err());
            store.cleanup(timestamp + week + 2000).unwrap();
            assert_eq!(store.range(&id, 0..3).unwrap(), b"raw");
            store.cleanup(timestamp + 2 * week + 4000).unwrap();
            assert!(store.range(&id, 0..3).is_err());
        }
    }

    #[test]
    fn every_engine_searches_full_titles_and_all_stored_tags() {
        let title = format!("{} Needle", "Long ".repeat(50));
        let long_tag = "a".repeat(80);
        let tags = (0..40)
            .map(|index| format!("#tag{index}"))
            .collect::<Vec<_>>()
            .join(" ");
        let body = format!("# {title}\n\n#{long_tag} {tags}");
        for engine in Engine::All.selected() {
            let directory = tempfile::tempdir().unwrap();
            let mut store = Storage::open(engine, directory.path()).unwrap();
            store.put("Long.md", &body, None).unwrap();
            for (query, titles) in [
                ("needle".to_owned(), true),
                (format!("#{long_tag}"), false),
                ("#tag39".into(), false),
            ] {
                assert_eq!(
                    store.search(&query, titles, 10).unwrap(),
                    vec!["Long.md"],
                    "{engine:?} {query}"
                );
            }
        }
    }

    #[test]
    fn portable_archives_preserve_sqlite_source_metadata_through_every_engine() {
        use tungsten::backup::{Kind, Reader};
        fn records(path: &std::path::Path) -> Vec<String> {
            let mut reader = Reader::open(path).unwrap();
            let mut values = Vec::new();
            while let Some(record) = reader.next_record().unwrap() {
                values.push(serde_json::to_string(&record).unwrap());
            }
            values.sort();
            values
        }
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source");
        let archive = directory.path().join("archive");
        let mut store = Storage::open(Engine::Sqlite, &source).unwrap();
        let revision = store.put("One.md", "# Thought", None).unwrap();
        let group = store.create_workspace("Research").unwrap();
        store.add_member(&group, "One.md").unwrap();
        let db = rusqlite::Connection::open(source.join("notes.sqlite")).unwrap();
        db.execute("INSERT INTO retitles(target,previous,title,revision,ready) VALUES('One.md','Before','Thought',?,0)", [revision.as_str()]).unwrap();
        drop(db);
        store.backup(&archive).unwrap();
        drop(store);
        let expected = records(&archive);
        assert!(expected.iter().any(|record| {
            serde_json::from_str::<tungsten::backup::Record>(record)
                .unwrap()
                .kind
                == Kind::TitleUpdate
        }));
        for engine in Engine::All.selected() {
            let output = directory.path().join(engine.name());
            Storage::restore(engine, &archive, &output).unwrap();
            let store = Storage::open(engine, &output).unwrap();
            let copy = directory.path().join(format!("{}-archive", engine.name()));
            store.backup(&copy).unwrap();
            assert_eq!(records(&copy), expected, "{engine:?}");
        }
    }

    #[test]
    fn indexed_queries_preserve_exact_titles_filters_and_updated_ordering() {
        for engine in Engine::All.selected() {
            let directory = tempfile::tempdir().unwrap();
            let mut store = Storage::open(engine, directory.path()).unwrap();
            let mut notes = Vec::new();
            for index in 0..24 {
                let name = format!("Note-{index:02}.md");
                let body = if index == 3 {
                    "# **Match** exact\n\n日本語 match #research - [ ] Task".to_owned()
                } else {
                    format!("# Topic {index}\n\n日本語 match #research\n\n- [ ] Task")
                };
                store.put(&name, &body, None).unwrap();
                notes.push(store.read(&name).unwrap());
            }
            for index in [3, 20] {
                store
                    .pin(notes[index].name.as_str(), &notes[index].revision)
                    .unwrap();
            }
            assert_eq!(
                store.search("match", false, 5).unwrap(),
                vec![
                    "Note-23.md",
                    "Note-22.md",
                    "Note-21.md",
                    "Note-20.md",
                    "Note-19.md"
                ],
                "{engine:?}"
            );
            assert_eq!(
                store.search("match exact", false, 5).unwrap(),
                vec!["Note-03.md"],
                "{engine:?}"
            );
            assert_eq!(
                store.search("is:pinned match", false, 5).unwrap(),
                vec!["Note-20.md", "Note-03.md"],
                "{engine:?}"
            );
            assert_eq!(
                store.search("-absent", false, 5).unwrap(),
                vec![
                    "Note-23.md",
                    "Note-22.md",
                    "Note-21.md",
                    "Note-20.md",
                    "Note-19.md"
                ],
                "{engine:?}"
            );
            notes.sort_by(|left, right| {
                (
                    matches!(right.name.as_str(), "Note-03.md" | "Note-20.md"),
                    right.modified,
                    right.name.as_str(),
                )
                    .cmp(&(
                        matches!(left.name.as_str(), "Note-03.md" | "Note-20.md"),
                        left.modified,
                        left.name.as_str(),
                    ))
            });
            let expected = notes
                .iter()
                .take(5)
                .map(|note| note.name.as_str())
                .collect::<Vec<_>>();
            for query in ["", "#research", "task:open", "path:note-"] {
                assert_eq!(
                    store.search(query, false, 5).unwrap(),
                    expected,
                    "{engine:?} {query}"
                );
            }
            let revision = store.read("Note-23.md").unwrap().revision;
            store
                .put("Note-23.md", "# Changed\n\n独立した本文", Some(&revision))
                .unwrap();
            assert!(
                !store
                    .search("match", false, 5)
                    .unwrap()
                    .contains(&"Note-23.md".to_owned()),
                "{engine:?}"
            );
        }
    }

    #[test]
    fn bulk_fixtures_remain_editable_and_preserve_ids_and_original_bodies() {
        use tungsten::{domain::NoteName, store::Note};
        for engine in Engine::All.selected() {
            let directory = tempfile::tempdir().unwrap();
            let mut store = Storage::open(engine, directory.path()).unwrap();
            let count = store
                .load_fixture((0..67).map(|index| {
                    let body = format!("# Thought {index}\n\n日本語");
                    Ok(Note {
                        name: NoteName::parse(format!("Note-{index:02}.md"))?,
                        revision: Revision::of(body.as_bytes()),
                        body,
                        modified: 1767225600000 + index,
                    })
                }))
                .unwrap();
            assert_eq!(count, 67);
            assert_eq!(
                store.search("日本", false, 3).unwrap(),
                vec!["Note-66.md", "Note-65.md", "Note-64.md"],
                "{engine:?}"
            );
            let note = store.read("Note-66.md").unwrap();
            store
                .put(
                    "Note-66.md",
                    "# Changed\n\nIndependent",
                    Some(&note.revision),
                )
                .unwrap();
            assert_eq!(store.history_count("Note-66.md").unwrap(), 1, "{engine:?}");
            let store = store.reopen().unwrap();
            assert_eq!(
                store.read("Note-66.md").unwrap().body,
                "# Changed\n\nIndependent"
            );
            assert_eq!(
                store.read("Note-00.md").unwrap().body,
                "# Thought 0\n\n日本語"
            );
        }
    }
}
