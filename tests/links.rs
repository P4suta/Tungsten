use tungsten::{domain::NoteName, store::Store};

#[test]
fn title_changes_refresh_default_link_labels_and_preserve_aliases_and_targets() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(directory.path()).unwrap();
    let target = store
        .create(Some("Topic.md".to_owned()), "# Philosophy\n\nBody")
        .unwrap();
    let source = store.create(Some("Notes/Source.md".to_owned()), "# Source\n\n[Philosophy](../Topic.md)\n[My reference](../Topic.md)\n`[Philosophy](../Topic.md)`\n\n[Other](../Other.md)\n").unwrap();
    store
        .update(
            &target.name,
            "# 思索 [notes]\n\nBody",
            target.revision.as_str(),
        )
        .unwrap();
    let linked = store.read(&source.name).unwrap();
    assert!(linked.body.contains("[思索 \\[notes\\]](../Topic.md)"));
    assert!(linked.body.contains("[My reference](../Topic.md)"));
    assert!(linked.body.contains("`[Philosophy](../Topic.md)`"));
    assert!(linked.body.contains("[Other](../Other.md)"));
    assert_ne!(linked.revision, source.revision);
    assert!(
        store
            .update(&source.name, "stale draft", source.revision.as_str())
            .is_err()
    );
    assert!(
        store
            .read(&NoteName::parse("Topic.md".to_owned()).unwrap())
            .is_ok()
    );
}
