use tungsten::{Error, domain::NoteName, store::Store};

#[test]
fn default_storage_is_a_keyless_database_with_transactional_indexes() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(directory.path()).unwrap();
    let note = store
        .create(Some("Note.md".into()), "# Written\n\nneedle")
        .unwrap();
    drop(store);
    assert!(directory.path().join("notes.sqlite").is_file());
    assert!(!directory.path().join("Note.md").exists());
    let store = Store::open(directory.path()).unwrap();
    assert_eq!(store.read(&note.name).unwrap().body, note.body);
    assert_eq!(store.search("needle", None, 10, false).unwrap().total, 1);
}

#[test]
fn failed_updates_roll_back_body_history_and_link_indexes_together() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(directory.path()).unwrap();
    let target = store.create(Some("Target.md".into()), "# Before").unwrap();
    let source = store
        .create(Some("Source.md".into()), "[Before](Target.md)")
        .unwrap();
    let database = rusqlite::Connection::open(directory.path().join("notes.sqlite")).unwrap();
    database.execute_batch("CREATE TRIGGER fail_save BEFORE UPDATE ON documents WHEN new.name='Target.md' BEGIN SELECT RAISE(ABORT,'injected failure'); END;").unwrap();
    assert!(
        store
            .update(&target.name, "# After", target.revision.as_str())
            .is_err()
    );
    assert_eq!(store.read(&target.name).unwrap().body, target.body);
    assert_eq!(store.read(&source.name).unwrap().body, source.body);
    assert!(store.history(&target.name).unwrap().is_empty());
    assert_eq!(
        store.search("Before", None, 10, true).unwrap().notes.len(),
        2
    );
    assert_eq!(
        store.search("After", None, 10, true).unwrap().notes.len(),
        0
    );
    drop(store);
    let store = Store::open(directory.path()).unwrap();
    assert_eq!(store.read(&target.name).unwrap().revision, target.revision);
}

#[test]
fn refuses_unknown_data_without_creating_an_empty_replacement() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("notes.sqlite");
    let database = rusqlite::Connection::open(&path).unwrap();
    database
        .execute_batch("CREATE TABLE unrelated(value TEXT); INSERT INTO unrelated VALUES('keep');")
        .unwrap();
    drop(database);
    assert!(Store::open(directory.path()).is_err());
    let database = rusqlite::Connection::open(path).unwrap();
    assert_eq!(
        database
            .query_row("SELECT value FROM unrelated", [], |row| row
                .get::<_, String>(0))
            .unwrap(),
        "keep"
    );
    assert!(matches!(
        NoteName::parse("../secret.md".into()),
        Err(Error::InvalidName)
    ));
}

#[test]
fn incomplete_backups_never_become_empty_databases() {
    for name in ["notes.sqlite", "notes.sqlite-wal", "notes.sqlite-journal"] {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join(name), []).unwrap();
        assert!(Store::open(directory.path()).is_err(), "{name}");
        assert_eq!(
            std::fs::metadata(directory.path().join(name))
                .unwrap()
                .len(),
            0
        );
    }
}

#[test]
fn missing_schema_contracts_do_not_silently_rebuild_a_database() {
    for statement in [
        "DROP TABLE documents",
        "DROP TRIGGER count_insert",
        "DROP INDEX incoming",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut store = Store::open(directory.path()).unwrap();
        store.create(Some("Keep.md".into()), "# Keep").unwrap();
        drop(store);
        let database = rusqlite::Connection::open(directory.path().join("notes.sqlite")).unwrap();
        database.execute_batch(statement).unwrap();
        drop(database);
        assert!(Store::open(directory.path()).is_err(), "{statement}");
    }
}

#[test]
fn stores_notes_below_a_symlinked_state_directory() {
    let directory = tempfile::tempdir().unwrap();
    let state = directory.path().join("private/tungsten");
    std::fs::create_dir_all(&state).unwrap();
    let alias = directory.path().join("tungsten");
    std::os::unix::fs::symlink(&state, &alias).unwrap();
    assert!(Store::open(&alias).is_err());
    let mut store = Store::open(&alias.join("notes")).unwrap();
    let note = store.create(Some("Saved.md".into()), "# Saved").unwrap();
    drop(store);
    let reopened = Store::open(&state.join("notes")).unwrap();
    assert_eq!(reopened.read(&note.name).unwrap().body, note.body);
    assert!(!state.join("notes.sqlite").exists());
}
