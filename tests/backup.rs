use rusqlite::{Connection, types::Value};
use std::{path::Path, process::Command};
use tungsten::store::Store;
use tungsten::{
    backup::{Cell, Kind, Record},
    domain::Revision,
};

fn command(data: &Path, argument: &str, path: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_tungsten"))
        .arg("--data-dir")
        .arg(data)
        .arg(argument)
        .arg(path)
        .output()
        .unwrap()
}

fn source_rows(data: &Path) -> Vec<(String, Vec<Vec<Value>>)> {
    let db = Connection::open(data.join("notes.sqlite")).unwrap();
    [
        "documents",
        "pins",
        "snapshots",
        "files",
        "gc_files",
        "retitles",
        "state",
        "workspaces",
        "workspace_members",
        "workspace_state",
        "sqlite_sequence",
        "retention_queue",
        "gc_dirty",
    ]
    .into_iter()
    .map(|table| {
        let count = db
            .prepare(&format!("SELECT * FROM {table}"))
            .unwrap()
            .column_count();
        let order = (1..=count)
            .map(|index| index.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let mut statement = db
            .prepare(&format!("SELECT * FROM {table} ORDER BY {order}"))
            .unwrap();
        let rows = statement
            .query_map([], |row| (0..count).map(|index| row.get(index)).collect())
            .unwrap()
            .collect::<Result<Vec<Vec<Value>>, _>>()
            .unwrap();
        (table.to_owned(), rows)
    })
    .collect()
}

#[test]
fn portable_backup_restores_originals_recovery_workspaces_and_maintenance() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source");
    let archive = directory.path().join("archive");
    let restored = directory.path().join("restored");
    let mut store = Store::open(&source).unwrap();
    let bytes: Vec<_> = (0..200_000).map(|index| (index % 251) as u8).collect();
    let attachment = store.upload("original.bin".into(), &bytes).unwrap();
    let first = store
        .create(Some("Nested/Note.md".into()), "# Before\n\n日本語")
        .unwrap();
    let note = store
        .update(
            &first.name,
            &format!("# After\n\n日本語\n[Original]({})", attachment.url),
            first.revision.as_str(),
        )
        .unwrap();
    let group = store.create_workspace("Research").unwrap();
    store.workspace_member(group.id, &note.name, true).unwrap();
    let deleted = store
        .create(Some("Deleted.md".into()), "# Keep in Trash")
        .unwrap();
    store
        .delete(&deleted.name, deleted.revision.as_str())
        .unwrap();
    assert!(!store.history(&note.name).unwrap().is_empty());
    drop(store);
    let before = source_rows(&source);
    let result = command(&source, "--backup-to", &archive);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(archive.join("notes/0000000000000001.md")).unwrap(),
        note.body
    );
    assert_eq!(
        std::fs::read(archive.join(format!("files/{}", attachment.id))).unwrap(),
        bytes
    );
    let result = command(&restored, "--restore-from", &archive);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(source_rows(&restored), before);
    let store = Store::open(&restored).unwrap();
    assert_eq!(store.read(&note.name).unwrap().revision, note.revision);
    assert_eq!(
        store.search("日本", None, 10, false).unwrap().notes.len(),
        1
    );
    assert_eq!(
        store
            .workspace_notes(group.id, None, 50)
            .unwrap()
            .notes
            .len(),
        1
    );
    assert_eq!(
        store.history(&note.name).unwrap()[0].revision,
        first.revision
    );
}

#[test]
fn corrupt_archives_never_publish_a_partial_database_or_replace_existing_data() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source");
    let archive = directory.path().join("archive");
    let restored = directory.path().join("restored");
    let mut store = Store::open(&source).unwrap();
    store.create(Some("Keep.md".into()), "# Original").unwrap();
    drop(store);
    assert!(command(&source, "--backup-to", &archive).status.success());
    std::fs::write(archive.join("notes/0000000000000001.md"), "corrupt").unwrap();
    assert!(
        !command(&restored, "--restore-from", &archive)
            .status
            .success()
    );
    assert!(!restored.exists());
    std::fs::create_dir(&restored).unwrap();
    std::fs::write(restored.join("keep"), "existing").unwrap();
    assert!(
        !command(&restored, "--restore-from", &archive)
            .status
            .success()
    );
    assert_eq!(
        std::fs::read_to_string(restored.join("keep")).unwrap(),
        "existing"
    );
    assert!(!command(&source, "--backup-to", &archive).status.success());
    assert_eq!(source_rows(&source)[0].1.len(), 1);
}

#[test]
fn checksummed_metadata_still_rejects_unsafe_names_missing_state_and_duplicates() {
    for defect in [
        "unsafe-name",
        "missing-state",
        "duplicate-note",
        "duplicate-sequence",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source");
        let archive = directory.path().join("archive");
        let restored = directory.path().join("restored");
        let mut store = Store::open(&source).unwrap();
        store.create(Some("Keep.md".into()), "# Keep").unwrap();
        drop(store);
        assert!(command(&source, "--backup-to", &archive).status.success());
        let metadata = std::fs::read_to_string(archive.join("metadata.jsonl")).unwrap();
        let mut records = metadata
            .lines()
            .map(|line| serde_json::from_str::<Record>(line).unwrap())
            .collect::<Vec<_>>();
        match defect {
            "unsafe-name" => {
                records
                    .iter_mut()
                    .find(|record| record.kind == Kind::Document)
                    .unwrap()
                    .values[0] = Cell::Text("../Escape.md".into());
            }
            "missing-state" => records.retain(|record| record.kind != Kind::State),
            "duplicate-note" => {
                records.push(
                    records
                        .iter()
                        .find(|record| record.kind == Kind::Document)
                        .unwrap()
                        .clone(),
                );
            }
            "duplicate-sequence" => records.push(
                records
                    .iter()
                    .find(|record| record.kind == Kind::Sequence)
                    .unwrap()
                    .clone(),
            ),
            _ => unreachable!(),
        }
        let mut bytes = Vec::new();
        for record in &records {
            serde_json::to_writer(&mut bytes, record).unwrap();
            bytes.push(b'\n');
        }
        let manifest = serde_json::json!({"format":1,"records":records.len(),"metadata_revision":Revision::of(&bytes)});
        std::fs::write(archive.join("metadata.jsonl"), &bytes).unwrap();
        std::fs::write(
            archive.join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        assert!(
            !command(&restored, "--restore-from", &archive)
                .status
                .success(),
            "{defect}"
        );
        assert!(!restored.exists());
        assert_eq!(Store::open(&source).unwrap().note_count().unwrap(), 1);
    }
}

#[test]
fn restoration_rejects_symlinked_payloads_and_truncated_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source");
    let archive = directory.path().join("archive");
    let restored = directory.path().join("restored");
    let mut store = Store::open(&source).unwrap();
    store.create(Some("Keep.md".into()), "# Keep").unwrap();
    drop(store);
    assert!(command(&source, "--backup-to", &archive).status.success());
    let payload = archive.join("notes/0000000000000001.md");
    let outside = directory.path().join("outside.md");
    std::fs::write(&outside, "# Keep").unwrap();
    std::fs::remove_file(&payload).unwrap();
    std::os::unix::fs::symlink(&outside, &payload).unwrap();
    assert!(
        !command(&restored, "--restore-from", &archive)
            .status
            .success()
    );
    assert!(!restored.exists());
    assert_eq!(std::fs::read_to_string(outside).unwrap(), "# Keep");
    std::fs::remove_file(&payload).unwrap();
    std::fs::write(&payload, "# Keep").unwrap();
    let metadata = archive.join("metadata.jsonl");
    let mut bytes = std::fs::read(&metadata).unwrap();
    bytes.pop();
    std::fs::write(metadata, bytes).unwrap();
    assert!(
        !command(&restored, "--restore-from", &archive)
            .status
            .success()
    );
    assert!(!restored.exists());
}

#[test]
fn restoration_preserves_exhausted_identity_and_workspace_counters() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source");
    let archive = directory.path().join("archive");
    let restored = directory.path().join("restored");
    drop(Store::open(&source).unwrap());
    let db = Connection::open(source.join("notes.sqlite")).unwrap();
    db.execute(
        "INSERT INTO sqlite_sequence(name,seq) VALUES('notes',?)",
        [i64::MAX - 1],
    )
    .unwrap();
    drop(db);
    let mut store = Store::open(&source).unwrap();
    let note = store.create(Some("Keep.md".into()), "# Keep").unwrap();
    let group = store.create_workspace("Research").unwrap();
    store.workspace_member(group.id, &note.name, true).unwrap();
    drop(store);
    let db = Connection::open(source.join("notes.sqlite")).unwrap();
    db.execute("UPDATE workspaces SET revision=?", [i64::MAX])
        .unwrap();
    db.execute("UPDATE workspace_state SET revision=?", [i64::MAX])
        .unwrap();
    drop(db);
    let before = source_rows(&source);
    let result = command(&source, "--backup-to", &archive);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let result = command(&restored, "--restore-from", &archive);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(source_rows(&restored), before);
    let store = Store::open(&restored).unwrap();
    for titles in [false, true] {
        let page = store.search("keep", None, 1, titles).unwrap();
        assert_eq!(page.notes.len(), 1);
        assert_eq!(page.notes[0].name, note.name);
        assert!(page.next.is_none());
    }
}

#[test]
fn backup_preserves_pending_titles_at_the_existing_note_limit() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source");
    let archive = directory.path().join("archive");
    let restored = directory.path().join("restored");
    let mut store = Store::open(&source).unwrap();
    let note = store.create(Some("One.md".into()), "# Thought").unwrap();
    drop(store);
    let db = Connection::open(source.join("notes.sqlite")).unwrap();
    db.execute(
        "INSERT INTO retitles(target,previous,title,revision,ready) VALUES('One.md',?,?,?,0)",
        rusqlite::params![
            "\u{1}".repeat(tungsten::store::MAX_NOTE_BYTES - 3),
            "\u{2}".repeat(tungsten::store::MAX_NOTE_BYTES - 3),
            note.revision.as_str()
        ],
    )
    .unwrap();
    drop(db);
    let before = source_rows(&source);
    let result = command(&source, "--backup-to", &archive);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let result = command(&restored, "--restore-from", &archive);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(source_rows(&restored), before);
}

#[test]
fn archives_preserve_names_that_collide_on_case_or_unicode_normalizing_filesystems() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source");
    let archive = directory.path().join("archive");
    let restored = directory.path().join("restored");
    let mut store = Store::open(&source).unwrap();
    for name in ["Case.md", "case.md", "Café.md", "Cafe\u{301}.md"] {
        store
            .create(Some(name.into()), &format!("# {name}"))
            .unwrap();
    }
    drop(store);
    let result = command(&source, "--backup-to", &archive);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let result = command(&restored, "--restore-from", &archive);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(source_rows(&restored), source_rows(&source));
}
