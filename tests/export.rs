use std::process::Command;
use tungsten::store::Store;

#[test]
fn exports_current_markdown_and_original_files_without_replacing_a_destination() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("notes");
    let output = directory.path().join("export");
    let mut store = Store::open(&source).unwrap();
    let bytes = vec![42; 200_000];
    let attachment = store.upload("original.bin".into(), &bytes).unwrap();
    let body = format!(
        "# Thought\n\n[Original]({})\n\n[Linked](../Other.md)",
        attachment.url
    );
    store.create(Some("Nested/Note.md".into()), &body).unwrap();
    store.create(Some("Other.md".into()), "# Other").unwrap();
    let trashed = store.create(Some("Trashed.md".into()), "deleted").unwrap();
    store
        .delete(&trashed.name, trashed.revision.as_str())
        .unwrap();
    drop(store);
    let export = || {
        Command::new(env!("CARGO_BIN_EXE_tungsten"))
            .arg("--data-dir")
            .arg(&source)
            .arg("--export-to")
            .arg(&output)
            .output()
            .unwrap()
    };
    let result = export();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(output.join("Nested/Note.md")).unwrap(),
        body
    );
    assert_eq!(
        std::fs::read(output.join(format!(".attachments/{}", attachment.id))).unwrap(),
        bytes
    );
    assert!(!output.join("Trashed.md").exists());
    assert!(!export().status.success());
    assert_eq!(
        std::fs::read_to_string(output.join("Other.md")).unwrap(),
        "# Other"
    );
    assert_eq!(Store::open(&source).unwrap().list("").unwrap().len(), 2);
}

#[test]
fn export_rejects_a_destination_inside_its_database_directory() {
    let directory = tempfile::tempdir().unwrap();
    drop(Store::open(directory.path()).unwrap());
    let output = directory.path().join("export");
    let result = Command::new(env!("CARGO_BIN_EXE_tungsten"))
        .arg("--data-dir")
        .arg(directory.path())
        .arg("--export-to")
        .arg(&output)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(!output.exists());
    assert!(Store::open(directory.path()).is_ok());
}

#[test]
fn exports_notes_with_deleted_and_missing_originals() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("notes");
    let output = directory.path().join("export");
    let mut store = Store::open(&source).unwrap();
    let kept = store.upload("kept.txt".into(), b"Kept bytes").unwrap();
    let deleted = store
        .upload("deleted.txt".into(), b"Deleted bytes")
        .unwrap();
    let missing = "00000000000000000000000000000000.txt";
    let body = format!(
        "# Originals\n\n[Kept]({})\n[Deleted]({})\n[Missing](.attachments/{missing})",
        kept.url, deleted.url
    );
    let note = store.create(Some("Originals.md".into()), &body).unwrap();
    store
        .create(Some("Shared.md".into()), &format!("[Kept]({})", kept.url))
        .unwrap();
    store
        .delete_attachment(&deleted.id, deleted.revision.as_str())
        .unwrap();
    drop(store);
    tungsten::export::write(&source, &output).unwrap();
    assert_eq!(
        std::fs::read_to_string(output.join("Originals.md")).unwrap(),
        body
    );
    assert_eq!(
        std::fs::read(output.join(".attachments").join(&kept.id)).unwrap(),
        b"Kept bytes"
    );
    assert!(!output.join(".attachments").join(&deleted.id).exists());
    assert!(!output.join(".attachments").join(missing).exists());
    assert_eq!(
        Store::open(&source).unwrap().read(&note.name).unwrap().body,
        body
    );
}
