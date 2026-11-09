use tungsten::{domain::NoteName, store::Store};

#[test]
fn encrypted_title_updates_survive_restart_and_renames_preserve_pins_and_history() {
    let directory = tempfile::tempdir().unwrap();
    let key = [119; 32];
    let mut store = Store::open_encrypted(directory.path(), &key).unwrap();
    let topic = store
        .create(Some("topic.md".into()), "# Topic\n\nOriginal thought")
        .unwrap();
    let source = store
        .create(
            Some("source.md".into()),
            "# Source\n\n[Topic](topic.md) and [Chosen alias](topic.md)",
        )
        .unwrap();
    store
        .pin(&source.name, true, source.revision.as_str())
        .unwrap();
    store
        .update(
            &topic.name,
            "# Revised topic\n\nChanged thought",
            topic.revision.as_str(),
        )
        .unwrap();
    drop(store);
    let mut store = Store::open_encrypted(directory.path(), &key).unwrap();
    for _ in 0..10 {
        if store.process_link_updates(256).unwrap() == 0 {
            break;
        }
    }
    let updated = store.read(&source.name).unwrap();
    assert!(updated.body.contains("[Revised topic](topic.md)"));
    assert!(updated.body.contains("[Chosen alias](topic.md)"));
    let renamed = NoteName::parse("renamed.md".into()).unwrap();
    store
        .rename(&source.name, &renamed, updated.revision.as_str())
        .unwrap();
    assert!(
        store
            .list("source")
            .unwrap()
            .iter()
            .any(|note| note.name == renamed && note.pinned)
    );
    assert!(
        store
            .graph(Some(&topic.name), 120)
            .unwrap()
            .edges
            .iter()
            .any(|edge| edge.source == renamed.as_str() && edge.target == topic.name.as_str())
    );
    assert!(!store.history(&renamed).unwrap().is_empty());
}

#[test]
fn encrypted_file_collection_waits_for_recovery_expiration_and_a_second_grace_period() {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open_encrypted(directory.path(), &[121; 32]).unwrap();
    let file = store
        .upload("preserved.txt".into(), b"Original bytes")
        .unwrap();
    let note = store
        .create(
            Some("note.md".into()),
            &format!("# File\n\n[Original]({})", file.url),
        )
        .unwrap();
    store
        .update(
            &note.name,
            "# File\n\nRemoved the link",
            note.revision.as_str(),
        )
        .unwrap();
    let later = SystemTime::now() + Duration::from_secs(8 * 86400);
    assert_eq!(
        store.collect_indexed_unused_attachments(later, 8).unwrap(),
        0
    );
    for _ in 0..10 {
        if store
            .process_recovery_cleanup(later.duration_since(UNIX_EPOCH).unwrap().as_millis(), 8)
            .unwrap()
            == 0
        {
            break;
        }
    }
    assert_eq!(
        store.collect_indexed_unused_attachments(later, 8).unwrap(),
        0
    );
    assert_eq!(store.attachment_bytes(&file.id).unwrap(), b"Original bytes");
    assert_eq!(
        store
            .collect_indexed_unused_attachments(later + Duration::from_secs(8 * 86400), 8)
            .unwrap(),
        1
    );
    assert!(store.attachment_bytes(&file.id).is_err());
}

#[test]
fn resumes_an_interrupted_encrypted_import_without_replacing_committed_notes() {
    let directory = tempfile::tempdir().unwrap();
    let key = [111; 32];
    let note = |index: usize| {
        let body = format!("# Entry {index}\n\nOriginal text");
        Ok(tungsten::store::Note {
            name: NoteName::parse(format!("{index:04}.md")).unwrap(),
            revision: tungsten::domain::Revision::of(body.as_bytes()),
            body,
            modified: 1_735_689_600_000,
        })
    };
    let mut store = Store::open_encrypted(directory.path(), &key).unwrap();
    assert!(
        store
            .import_encrypted((0..510).map(|index| if index == 501 {
                Err(tungsten::Error::Worker)
            } else {
                note(index)
            }))
            .is_err()
    );
    drop(store);
    let mut resumed = Store::open_encrypted(directory.path(), &key).unwrap();
    assert_eq!(resumed.import_encrypted((500..510).map(note)).unwrap(), 510);
    assert_eq!(resumed.list("original").unwrap().len(), 200);
    assert_eq!(resumed.search("", None, 1, false).unwrap().total, 510);
    assert_eq!(resumed.import_encrypted(std::iter::empty()).unwrap(), 510);
    assert!(resumed.import_encrypted(std::iter::once(note(0))).is_err());
}

#[test]
fn deleted_originals_release_database_space_in_bounded_maintenance_batches() {
    let directory = tempfile::tempdir().unwrap();
    let key = [97; 32];
    let mut store = Store::open_encrypted(directory.path(), &key).unwrap();
    store
        .upload("temporary.bin".into(), &vec![42; 2 * 1024 * 1024])
        .unwrap();
    drop(store);
    let path = directory.path().join(".sealed.sqlite");
    let before = std::fs::metadata(&path).unwrap().len();
    let database = rusqlite::Connection::open(&path).unwrap();
    database
        .pragma_update(None, "key", format!("x'{}'", "61".repeat(32)))
        .unwrap();
    assert_eq!(
        database
            .pragma_query_value(None, "auto_vacuum", |row| row.get::<_, i64>(0))
            .unwrap(),
        2
    );
    drop(database);
    let mut store = Store::open_encrypted(directory.path(), &key).unwrap();
    assert_eq!(
        store
            .collect_indexed_unused_attachments(
                std::time::SystemTime::now() + std::time::Duration::from_secs(8 * 86400),
                8
            )
            .unwrap(),
        1
    );
    let mut batches = 0;
    loop {
        let reclaimed = store.reclaim_deleted_pages().unwrap();
        assert!(reclaimed <= 32);
        if reclaimed == 0 {
            break;
        }
        batches += 1;
        assert!(batches < 1000);
    }
    assert!(batches > 1);
    drop(store);
    assert!(std::fs::metadata(path).unwrap().len() < before / 2);
}

#[test]
fn encrypted_storage_preserves_notes_search_history_trash_and_original_files() {
    let directory = tempfile::tempdir().unwrap();
    let key = [73; 32];
    let name = NoteName::parse("Private title.md".to_owned()).unwrap();
    let mut store = Store::open_encrypted(directory.path(), &key).unwrap();
    let file = store
        .upload("private original.txt".into(), b"private original bytes")
        .unwrap();
    let body = format!(
        "# Private title\n\nPrivate philosophy 日本語\n\n[file]({})",
        file.url
    );
    let initial = store.create(Some(name.as_str().into()), &body).unwrap();
    let edited = store
        .update(
            &name,
            "# Revised title\n\nPrivate thoughts",
            initial.revision.as_str(),
        )
        .unwrap();
    assert_eq!(store.list("日本語").unwrap().len(), 0);
    assert_eq!(store.list("thoughts").unwrap().len(), 1);
    let version = store.history(&name).unwrap().remove(0);
    assert_eq!(store.saved_note(&version.id, false).unwrap().body, body);
    store.delete(&name, edited.revision.as_str()).unwrap();
    let trashed = store.trash().unwrap().remove(0);
    assert_eq!(
        store.saved_note(&trashed.id, true).unwrap().body,
        edited.body
    );
    store.restore_trash(&trashed.id, None).unwrap();
    assert_eq!(
        store.attachment_bytes(&file.id).unwrap(),
        b"private original bytes"
    );
    drop(store);
    assert!(Store::open_encrypted(directory.path(), &[74; 32]).is_err());
    let reopened = Store::open_encrypted(directory.path(), &key).unwrap();
    assert_eq!(reopened.read(&name).unwrap().body, edited.body);
    assert_eq!(reopened.list("Private").unwrap().len(), 1);
    for entry in std::fs::read_dir(directory.path()).unwrap() {
        let entry = entry.unwrap();
        assert!(entry.file_type().unwrap().is_file());
        let bytes = std::fs::read(entry.path()).unwrap();
        for secret in [
            "Private title",
            "Private philosophy",
            "Private thoughts",
            "private original",
            "日本語",
        ] {
            assert!(
                !bytes
                    .windows(secret.len())
                    .any(|part| part == secret.as_bytes())
            );
        }
        assert!(!bytes.starts_with(b"SQLite format 3"));
    }
}

#[test]
fn searches_encrypted_originals_with_unicode_case_and_negative_substrings() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open_encrypted(directory.path(), &[109; 32]).unwrap();
    store
        .create(Some("first.md".into()), "# First\n\nÉCOLE café 日本語")
        .unwrap();
    store
        .create(Some("second.md".into()), "# Second\n\nCafé 日本語")
        .unwrap();
    assert_eq!(store.list("école").unwrap().len(), 1);
    assert_eq!(store.list("café -école").unwrap().len(), 1);
    assert_eq!(store.list("日").unwrap().len(), 2);
    assert_eq!(store.list("-école").unwrap().len(), 1);
}
#[test]
fn failed_recovery_deletion_rolls_back_the_body_and_its_catalog_entries() {
    let directory = tempfile::tempdir().unwrap();
    let key = [79; 32];
    let mut store = Store::open_encrypted(directory.path(), &key).unwrap();
    let note = store
        .create(
            Some("Recoverable.md".into()),
            "# Recoverable\n\nKeep the original body.",
        )
        .unwrap();
    store.delete(&note.name, note.revision.as_str()).unwrap();
    let saved = store.trash().unwrap().remove(0);
    drop(store);
    let database = rusqlite::Connection::open(directory.path().join(".sealed.sqlite")).unwrap();
    database
        .pragma_update(None, "key", format!("x'{}'", "4f".repeat(32)))
        .unwrap();
    database.execute_batch("CREATE TRIGGER fail_record_removal BEFORE DELETE ON records BEGIN SELECT RAISE(ABORT,'injected deletion failure'); END;").unwrap();
    drop(database);
    let mut reopened = Store::open_encrypted(directory.path(), &key).unwrap();
    assert!(
        reopened
            .purge_trash(&saved.id, saved.revision.as_str())
            .is_err()
    );
    assert_eq!(
        reopened.saved_note(&saved.id, true).unwrap().body,
        note.body
    );
    assert_eq!(reopened.trash().unwrap().len(), 1);
}

#[test]
fn live_journals_remain_encrypted_and_corruption_never_triggers_plaintext_repair() {
    let directory = tempfile::tempdir().unwrap();
    let key = [89; 32];
    let mut store = Store::open_encrypted(directory.path(), &key).unwrap();
    let note = store
        .create(
            Some("Private filename.md".into()),
            "# Private title\n\nPrivate indexed 日本語 content.",
        )
        .unwrap();
    store
        .update(
            &note.name,
            "# Private title\n\nChanged private body.",
            note.revision.as_str(),
        )
        .unwrap();
    store
        .upload("Private original.bin".into(), b"Private original contents.")
        .unwrap();
    assert!(directory.path().join(".sealed.sqlite-wal").exists());
    for entry in std::fs::read_dir(directory.path()).unwrap() {
        let bytes = std::fs::read(entry.unwrap().path()).unwrap();
        for text in [
            "Private filename",
            "Private title",
            "Private indexed",
            "private body",
            "Private original",
            "日本語",
            &"59".repeat(32),
        ] {
            assert!(
                !bytes
                    .windows(text.len())
                    .any(|part| part == text.as_bytes())
            );
        }
    }
    drop(store);
    let path = directory.path().join(".sealed.sqlite");
    let mut corrupt = std::fs::read(&path).unwrap();
    corrupt[64] ^= 1;
    std::fs::write(&path, &corrupt).unwrap();
    assert!(Store::open_encrypted(directory.path(), &key).is_err());
    assert!(Store::open(directory.path()).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), corrupt);
    assert!(!directory.path().join(".index").exists());
}
