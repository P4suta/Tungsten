use std::time::{Duration, SystemTime};
use tungsten::{domain::NoteName, store::Store};

#[test]
fn cleanup_keeps_recent_uploads_and_all_recovery_references() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(directory.path()).unwrap();
    let orphan = store.upload("unused.txt".to_owned(), b"unused").unwrap();
    let live = store.upload("live.txt".to_owned(), b"live").unwrap();
    let history = store.upload("history.txt".to_owned(), b"history").unwrap();
    let trash = store.upload("trash.txt".to_owned(), b"trash").unwrap();
    for (name, id, disposition) in [
        ("Nested/Live.md", &live.id, 0),
        ("History.md", &history.id, 1),
        ("Trash.md", &trash.id, 2),
    ] {
        let note = store
            .create(
                Some(name.to_owned()),
                &format!("[file](../.attachments/{id})"),
            )
            .unwrap();
        match disposition {
            1 => {
                store
                    .update(&note.name, "replacement", note.revision.as_str())
                    .unwrap();
            }
            2 => store.delete(&note.name, note.revision.as_str()).unwrap(),
            _ => {}
        }
    }
    let now = SystemTime::now();
    assert_eq!(store.collect_unused_attachments(now).unwrap(), 0);
    let later = now + Duration::from_secs(7 * 24 * 60 * 60 + 1);
    assert_eq!(store.collect_unused_attachments(later).unwrap(), 1);
    assert!(store.attachment_bytes(&orphan.id).is_err());
    for id in [&live.id, &history.id, &trash.id] {
        assert!(store.attachment_bytes(id).is_ok());
    }
    assert!(
        store
            .history(&NoteName::parse("History.md".to_owned()).unwrap())
            .unwrap()
            .is_empty()
    );
    assert!(store.trash().unwrap().is_empty());
    assert_eq!(store.collect_unused_attachments(later).unwrap(), 0);
    assert_eq!(
        store
            .collect_unused_attachments(later + Duration::from_secs(7 * 24 * 60 * 60))
            .unwrap(),
        2
    );
    assert!(store.attachment_bytes(&live.id).is_ok());
}

#[test]
fn cleanup_aborts_before_deleting_when_a_recovery_body_is_invalid() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(directory.path()).unwrap();
    let orphan = store.upload("keep.txt".into(), b"keep").unwrap();
    let note = store.create(Some("Record.md".into()), "saved").unwrap();
    store.delete(&note.name, note.revision.as_str()).unwrap();
    let record = store.trash().unwrap().remove(0);
    let database = rusqlite::Connection::open(directory.path().join("notes.sqlite")).unwrap();
    database
        .execute(
            "UPDATE snapshots SET body='corrupt' WHERE trash=1 AND id=?",
            [&record.id],
        )
        .unwrap();
    let later = SystemTime::now() + Duration::from_secs(8 * 86400);
    assert!(store.collect_unused_attachments(later).is_err());
    assert_eq!(store.attachment_bytes(&orphan.id).unwrap(), b"keep");
    database
        .execute(
            "UPDATE snapshots SET body='saved' WHERE trash=1 AND id=?",
            [&record.id],
        )
        .unwrap();
    assert_eq!(store.collect_unused_attachments(later).unwrap(), 1);
}

#[test]
fn restored_references_reset_the_full_grace_period_and_cleanup_ignores_external_paths() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(directory.path()).unwrap();
    let file = store.upload("recover.txt".to_owned(), b"recover").unwrap();
    let now = SystemTime::now();
    store.collect_unused_attachments(now).unwrap();
    let note = store
        .create(
            Some("Recover.md".to_owned()),
            &format!("[recover](.attachments/{})", file.id),
        )
        .unwrap();
    let day = Duration::from_secs(24 * 60 * 60);
    store.collect_unused_attachments(now + day * 6).unwrap();
    store.delete(&note.name, note.revision.as_str()).unwrap();
    let trash = store.trash().unwrap().remove(0);
    store
        .purge_trash(&trash.id, trash.revision.as_str())
        .unwrap();
    assert_eq!(store.collect_unused_attachments(now + day * 8).unwrap(), 0);
    assert_eq!(store.collect_unused_attachments(now + day * 14).unwrap(), 0);
    assert_eq!(store.collect_unused_attachments(now + day * 15).unwrap(), 1);
    let outside = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(outside.path(), b"outside").unwrap();
    let id = "00000000000000000000000000000000.txt";
    std::os::unix::fs::symlink(outside.path(), directory.path().join(id)).unwrap();
    store.collect_unused_attachments(now + day * 20).unwrap();
    store.collect_unused_attachments(now + day * 30).unwrap();
    assert_eq!(std::fs::read(outside.path()).unwrap(), b"outside");
}
