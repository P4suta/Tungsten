use std::time::{Duration, SystemTime};
use tungsten::store::Store;

#[test]
fn indexed_collection_is_bounded_and_references_restart_the_grace_period() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(directory.path()).unwrap();
    let now = SystemTime::now();
    let mut files = Vec::new();
    for index in 0..35 {
        files.push(
            store
                .upload(format!("sample-{index}.txt"), b"original")
                .unwrap(),
        );
    }
    let note = store
        .create(
            Some("keep.md".into()),
            &format!("# Keep\n\n[file](.attachments/{}#fragment)", files[0].id),
        )
        .unwrap();
    let day = Duration::from_secs(86400);
    assert_eq!(
        store
            .collect_indexed_unused_attachments(now + day * 8, 10)
            .unwrap(),
        10
    );
    assert_eq!(store.attachment_bytes(&files[0].id).unwrap(), b"original");
    store.delete(&note.name, note.revision.as_str()).unwrap();
    let saved = store.trash().unwrap().remove(0);
    store
        .purge_trash(&saved.id, saved.revision.as_str())
        .unwrap();
    store
        .collect_indexed_unused_attachments(now + day * 9, 50)
        .unwrap();
    store
        .collect_indexed_unused_attachments(now + day * 15, 50)
        .unwrap();
    assert_eq!(store.attachment_bytes(&files[0].id).unwrap(), b"original");
    store
        .collect_indexed_unused_attachments(now + day * 17, 50)
        .unwrap();
    assert!(store.attachment_bytes(&files[0].id).is_err());
}

#[test]
fn automatic_collection_stops_after_reopening_unreadable_sources() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(directory.path()).unwrap();
    let file = store.upload("keep.txt".into(), b"original").unwrap();
    drop(store);
    std::fs::write(directory.path().join("broken.md"), [0xff]).unwrap();
    let mut store = Store::open(directory.path()).unwrap();
    assert!(
        store
            .collect_indexed_unused_attachments(
                SystemTime::now() + Duration::from_secs(8 * 86400),
                8
            )
            .is_err()
    );
    assert_eq!(store.attachment_bytes(&file.id).unwrap(), b"original");
    drop(store);
    std::fs::remove_file(directory.path().join("broken.md")).unwrap();
    let mut store = Store::open(directory.path()).unwrap();
    assert_eq!(
        store
            .collect_indexed_unused_attachments(
                SystemTime::now() + Duration::from_secs(8 * 86400),
                8
            )
            .unwrap(),
        1
    );
}
