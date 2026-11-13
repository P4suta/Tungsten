use tungsten_linux::{
    Error, NoteName, Revision,
    drafts::{Draft, DraftStore, PendingWrite},
    editor::Editor,
    types::{Note, ServerAddress},
};
use tungsten_linux_model::Block;

fn server() -> ServerAddress {
    ServerAddress::parse("https://notes.example", false).unwrap()
}
fn note(body: &str) -> Note {
    Note {
        name: NoteName::parse("Existing.md".into()).unwrap(),
        body: body.into(),
        revision: Revision::of(body.as_bytes()),
    }
}

#[test]
fn history_reads_refuse_edits_even_if_text_returns_to_its_earlier_value() {
    let directory = tempfile::tempdir().unwrap();
    let store = DraftStore::at(directory.path().to_owned());
    let mut editor = Editor::loaded(server(), note("Original"), 1);
    let read = editor.history_read();
    editor.edit("Concurrent".into(), &store).unwrap();
    assert!(matches!(
        editor.replace_from_history(&read, "Historical".into(), &store),
        Err(Error::Busy)
    ));
    editor.edit("Original".into(), &store).unwrap();
    assert!(matches!(
        editor.replace_from_history(&read, "Historical".into(), &store),
        Err(Error::Busy)
    ));
    let read = editor.history_read();
    editor
        .replace_from_history(&read, "Historical".into(), &store)
        .unwrap();
    assert_eq!(editor.draft().body, "Historical");
    assert_eq!(editor.draft().revision, Some(note("Original").revision));
}

#[test]
fn failed_history_replacement_keeps_visible_writing_until_it_is_durable_again() {
    let directory = tempfile::tempdir().unwrap();
    let store = DraftStore::at(directory.path().join("drafts"));
    let blocked = directory.path().join("blocked");
    std::fs::write(&blocked, b"unavailable").unwrap();
    let unavailable = DraftStore::at(blocked);
    let mut editor = Editor::loaded(server(), note("Original"), 1);
    editor.edit("Visible writing".into(), &store).unwrap();
    let read = editor.history_read();
    assert!(
        editor
            .replace_from_history(&read, "Historical".into(), &unavailable)
            .is_err()
    );
    assert_eq!(editor.draft().body, "Visible writing");
    assert!(!editor.state().may_leave());
    editor.persist(&store).unwrap();
    assert!(editor.state().may_leave());
    assert_eq!(
        store.recover(&server()).unwrap().drafts[0].body,
        "Visible writing"
    );
    assert!(editor.edit("Typed writing".into(), &unavailable).is_err());
    assert_eq!(editor.draft().body, "Typed writing");
    assert!(!editor.state().may_leave());
}

#[test]
fn history_sync_failure_after_replacement_recovers_the_visible_draft() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let directory = tempfile::tempdir().unwrap();
    let store = DraftStore::at(directory.path().to_owned());
    let mut editor = Editor::loaded(server(), note("Original"), 1);
    editor.edit("Visible writing".into(), &store).unwrap();
    let replaced = Arc::new(AtomicBool::new(false));
    let observed = replaced.clone();
    let interrupted = DraftStore::with_directory_sync(directory.path().to_owned(), move |path| {
        assert!(std::fs::read_dir(path)?.next().is_some());
        observed.store(true, Ordering::SeqCst);
        Err(std::io::Error::other(
            "directory sync failed after replacement",
        ))
    });
    let read = editor.history_read();
    assert!(
        editor
            .replace_from_history(&read, "Historical".into(), &interrupted)
            .is_err()
    );
    assert!(replaced.load(Ordering::SeqCst));
    assert_eq!(editor.draft().body, "Visible writing");
    assert!(!editor.state().may_leave());
    drop(editor);
    let restarted = DraftStore::at(directory.path().to_owned());
    assert_eq!(
        restarted.recover(&server()).unwrap().drafts[0].body,
        "Visible writing"
    );
}

#[test]
fn history_replacement_stays_unsaved_until_the_editor_presents_it() {
    let directory = tempfile::tempdir().unwrap();
    let store = DraftStore::at(directory.path().to_owned());
    let mut editor = Editor::loaded(server(), note("Original"), 1);
    editor.edit("Visible writing".into(), &store).unwrap();
    let read = editor.history_read();
    editor
        .replace_from_history(&read, "Historical".into(), &store)
        .unwrap();
    assert_eq!(editor.draft().body, "Historical");
    assert!(!editor.state().may_leave());
    assert_eq!(
        store.recover(&server()).unwrap().drafts[0].body,
        "Visible writing"
    );
    assert!(editor.begin(&store).unwrap().is_none());
    editor.persist(&store).unwrap();
    assert_eq!(
        store.recover(&server()).unwrap().drafts[0].body,
        "Historical"
    );
    assert!(editor.state().may_leave());
}

#[test]
fn retry_only_creates_missing_notes_without_an_acknowledged_or_pending_write() {
    let directory = tempfile::tempdir().unwrap();
    let store = DraftStore::at(directory.path().to_owned());
    for acknowledged in [false, true] {
        for pending in [false, true] {
            let revision = acknowledged.then(|| note("Original").revision);
            let draft = Draft {
                format: 1,
                server: server(),
                name: note("").name,
                body: "Local writing".into(),
                revision: revision.clone(),
                pending: pending.then(|| PendingWrite {
                    body: "Earlier write".into(),
                    revision,
                }),
            };
            let mut editor = Editor::recover(draft, 1);
            let result = editor.reconcile(Err(Error::Missing), &store);
            if acknowledged || pending {
                assert!(matches!(result, Err(Error::Missing)));
                assert_eq!(editor.state().block(), Block::Missing);
                assert!(editor.begin(&store).unwrap().is_none());
            } else {
                result.unwrap();
                assert_eq!(editor.state().block(), Block::None);
                assert!(editor.begin(&store).unwrap().is_some());
            }
            assert_eq!(editor.draft().body, "Local writing");
        }
    }
}

#[test]
fn delayed_acknowledgements_and_failures_cannot_change_a_replacement_editor() {
    let directory = tempfile::tempdir().unwrap();
    let store = DraftStore::at(directory.path().to_owned());
    let mut earlier = Editor::create(server(), 1);
    earlier.edit("Earlier".into(), &store).unwrap();
    let (ticket, outgoing) = earlier.begin(&store).unwrap().unwrap();
    let mut current = Editor::loaded(server(), note("Current"), 2);
    let response = Note {
        name: outgoing.name,
        body: outgoing.body,
        revision: Revision::of(b"Earlier"),
    };
    assert!(!current.complete(ticket, Ok(response), &store).unwrap());
    assert!(
        !current
            .complete(ticket, Err(Error::Conflict), &store)
            .unwrap()
    );
    assert_eq!(current.draft().body, "Current");
    assert!(current.state().is_saved());
}

#[test]
fn failed_revision_persistence_keeps_the_last_outgoing_write_recoverable() {
    let directory = tempfile::tempdir().unwrap();
    let store = DraftStore::at(directory.path().join("drafts"));
    let mut editor = Editor::create(server(), 1);
    editor.edit("First".into(), &store).unwrap();
    let (ticket, outgoing) = editor.begin(&store).unwrap().unwrap();
    editor.edit("Later".into(), &store).unwrap();
    let folder = std::fs::read_dir(directory.path().join("drafts"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let archive = directory.path().join("kept");
    std::fs::rename(&folder, &archive).unwrap();
    std::fs::write(&folder, b"unavailable").unwrap();
    let acknowledged = Note {
        name: outgoing.name,
        body: outgoing.body,
        revision: Revision::of(b"First"),
    };
    assert!(
        editor
            .complete(ticket, Ok(acknowledged.clone()), &store)
            .is_err()
    );
    assert_eq!(editor.draft().body, "Later");
    std::fs::remove_file(&folder).unwrap();
    std::fs::rename(archive, folder).unwrap();
    let draft = store.recover(&server()).unwrap().drafts.pop().unwrap();
    assert_eq!(draft.body, "Later");
    assert_eq!(draft.pending.as_ref().unwrap().body, "First");
    let mut recovered = Editor::recover(draft, 2);
    recovered
        .reconcile(Ok(acknowledged.clone()), &store)
        .unwrap();
    assert_eq!(recovered.draft().revision, Some(acknowledged.revision));
    assert_eq!(recovered.draft().body, "Later");
}

#[test]
fn corrupt_identity_files_survive_removal_and_replacement() {
    let directory = tempfile::tempdir().unwrap();
    let store = DraftStore::at(directory.path().to_owned());
    let mut editor = Editor::create(server(), 1);
    editor.edit("Protected".into(), &store).unwrap();
    let folder = std::fs::read_dir(directory.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let path = std::fs::read_dir(&folder)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::write(&path, b"broken").unwrap();
    store.remove(&server(), &editor.draft().name).unwrap();
    assert_eq!(store.recover(&server()).unwrap().damaged.len(), 1);
    editor.persist(&store).unwrap();
    let recovered = store.recover(&server()).unwrap();
    assert_eq!(recovered.damaged.len(), 1);
    assert_eq!(recovered.drafts.len(), 1);
    assert_eq!(std::fs::read(&recovered.damaged[0]).unwrap(), b"broken");
}

#[test]
fn server_preferences_are_atomic_and_new_draft_files_are_private() {
    let directory = tempfile::tempdir().unwrap();
    let store = DraftStore::at(directory.path().join("app/drafts"));
    assert!(store.remembered_server().unwrap().is_none());
    store.remember_server(&server()).unwrap();
    assert_eq!(store.remembered_server().unwrap(), Some(server()));
    let draft = Draft {
        format: 1,
        server: server(),
        name: note("").name,
        body: "Private".into(),
        revision: None,
        pending: None,
    };
    store.save(&draft).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let root = directory.path().join("app/drafts");
        assert_eq!(
            std::fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let record = std::fs::read_dir(&root)
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| entry.file_type().unwrap().is_dir())
            .unwrap();
        let file = std::fs::read_dir(record.path())
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert_eq!(
            std::fs::metadata(file).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
