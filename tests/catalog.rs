use tungsten::{domain::NoteName, store::Store};

#[test]
fn searches_page_without_losing_substrings_unicode_or_filter_semantics() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(directory.path()).unwrap();
    for index in 0..150 {
        store
            .create(
                Some(format!("{index:04}.md")),
                &format!(
                    "# Entry {index}\n\nhogepiyo 日本語 café 👩\u{200d}💻 #work\n- [ ] task\n"
                ),
            )
            .unwrap();
    }
    let mut names = std::collections::BTreeSet::new();
    let mut cursor = None;
    loop {
        let page = store
            .search("gepi 日本", cursor.as_deref(), 17, false)
            .unwrap();
        assert!(page.notes.len() <= 17);
        for note in page.notes {
            assert!(names.insert(note.name.as_str().to_owned()));
        }
        cursor = page.next;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(names.len(), 150);
    let page = store.search("Entry 1", None, 7, false).unwrap();
    assert_eq!(page.notes[0].title, "Entry 1");
    let mut found = page
        .notes
        .into_iter()
        .map(|note| note.name.as_str().to_owned())
        .collect::<std::collections::BTreeSet<_>>();
    let mut cursor = page.next;
    while let Some(next) = cursor {
        let page = store.search("Entry 1", Some(&next), 7, false).unwrap();
        for note in page.notes {
            assert!(found.insert(note.name.as_str().to_owned()));
        }
        cursor = page.next;
    }
    assert_eq!(found.len(), 69);
    for query in [
        "日",
        "日本",
        "café",
        "👩\u{200d}💻",
        "#work",
        "task:open",
        "-absent",
    ] {
        assert_eq!(
            store.search(query, None, 20, false).unwrap().notes.len(),
            20,
            "{query}"
        );
    }
    assert!(
        store
            .search("hogeZZpiyo", None, 20, false)
            .unwrap()
            .notes
            .is_empty()
    );
    assert!(
        store
            .search("-hogepiyo", None, 20, false)
            .unwrap()
            .notes
            .is_empty()
    );
    assert!(store.search("#work", None, 0, false).is_err());
    assert!(store.search("#work", Some("invalid"), 20, false).is_err());
    let cursor = store
        .search("hogepiyo", None, 10, false)
        .unwrap()
        .next
        .unwrap();
    assert!(store.search("other", Some(&cursor), 10, false).is_err());
}

#[test]
fn catalog_tracks_writes_deletes_restores_and_external_changes_on_restart() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(directory.path()).unwrap();
    let note = store
        .create(Some("a.md".into()), "# First\n\nneedle")
        .unwrap();
    let updated = store
        .update(
            &note.name,
            "# Second\n\nreplacement",
            note.revision.as_str(),
        )
        .unwrap();
    assert!(store.list("needle").unwrap().is_empty());
    assert_eq!(store.list("replacement").unwrap().len(), 1);
    store
        .delete(&updated.name, updated.revision.as_str())
        .unwrap();
    assert!(store.list("replacement").unwrap().is_empty());
    let trash = store.trash().unwrap().remove(0);
    store.restore_trash(&trash.id, None).unwrap();
    assert_eq!(store.list("replacement").unwrap().len(), 1);
    drop(store);
    std::fs::write(
        directory.path().join("a.md"),
        "# External\n\nchanged outside",
    )
    .unwrap();
    std::fs::write(directory.path().join("b.md"), "# Another\n\nnew outside").unwrap();
    let store = Store::open(directory.path()).unwrap();
    assert!(store.list("replacement").unwrap().is_empty());
    assert_eq!(store.list("outside").unwrap().len(), 2);
    assert_eq!(
        store
            .read(&NoteName::parse("a.md".into()).unwrap())
            .unwrap()
            .body,
        "# External\n\nchanged outside"
    );
}

#[test]
fn graph_neighborhoods_include_incoming_links_without_transferring_every_note() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(directory.path()).unwrap();
    let center = store.create(Some("center.md".into()), "# Center").unwrap();
    for index in 0..250 {
        store
            .create(
                Some(format!("source-{index}.md")),
                &format!("# Source {index}\n\n[Center](center.md)"),
            )
            .unwrap();
    }
    let graph = store.graph(Some(&center.name), 20).unwrap();
    assert_eq!(graph.total, 251);
    assert_eq!(graph.neighbors, 250);
    assert_eq!(graph.notes.len(), 20);
    assert_eq!(graph.edges.len(), 19);
    assert!(graph.notes.iter().any(|note| note.name == center.name));
    assert!(store.graph(None, 121).is_err());
}

#[test]
fn recovery_lists_are_bounded_and_do_not_read_unrelated_bodies() {
    use tungsten::domain::Revision;
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("a.md"), "# Current").unwrap();
    let captured = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    for index in 0..600 {
        let body = format!("# Version {index}\n");
        let name = if index % 2 == 0 { "a.md" } else { "foreign.md" };
        let record = directory.path().join(format!(".history/{index:032x}"));
        std::fs::create_dir_all(&record).unwrap();
        std::fs::write(record.join("body.md"), &body).unwrap();
        std::fs::write(record.join("metadata.json"),serde_json::to_vec(&serde_json::json!({"name":name,"modified":captured,"captured":captured-index,"revision":Revision::of(body.as_bytes()).as_str()})).unwrap()).unwrap();
    }
    let mut store = Store::open(directory.path()).unwrap();
    let name = NoteName::parse("a.md".into()).unwrap();
    assert_eq!(store.history(&name).unwrap().len(), 50);
    std::fs::write(
        directory
            .path()
            .join(format!(".history/{:032x}/body.md", 1)),
        "corrupt unrelated body",
    )
    .unwrap();
    let history = store.history(&name).unwrap();
    assert_eq!(history.len(), 50);
    assert_eq!(history[0].id, format!("{:032x}", 0));
    assert_eq!(
        store.saved_note(&history[0].id, false).unwrap().body,
        "# Version 0\n"
    );
    assert!(
        store
            .collect_unused_attachments(std::time::SystemTime::now())
            .is_err()
    );
}

#[test]
fn queued_title_updates_survive_restarts_and_chained_title_edits() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(directory.path()).unwrap();
    let target = store.create(Some("target.md".into()), "# First").unwrap();
    for index in 0..30 {
        store
            .create(
                Some(format!("source-{index}.md")),
                "# Source\n\n[First](target.md)\n[Alias](target.md)",
            )
            .unwrap();
    }
    let target = store
        .update(&target.name, "# Second", target.revision.as_str())
        .unwrap();
    store
        .update(&target.name, "# Third", target.revision.as_str())
        .unwrap();
    drop(store);
    let mut store = Store::open(directory.path()).unwrap();
    for _ in 0..62 {
        store.process_link_updates(8).unwrap();
    }
    for index in 0..30 {
        let source = store
            .read(&NoteName::parse(format!("source-{index}.md")).unwrap())
            .unwrap();
        assert!(source.body.contains("[Third](target.md)"));
        assert!(source.body.contains("[Alias](target.md)"));
    }
}

#[test]
fn emptying_trash_is_durable_and_keeps_later_deletions_restorable() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(directory.path()).unwrap();
    for index in 0..65 {
        let note = store
            .create(Some(format!("trash-{index}.md")), "# Trash")
            .unwrap();
        store.delete(&note.name, note.revision.as_str()).unwrap();
    }
    let old = store.trash().unwrap()[0].id.clone();
    store.empty_trash().unwrap();
    assert!(store.trash().unwrap().is_empty());
    assert!(store.saved_note(&old, true).is_err());
    let later = store.create(Some("later.md".into()), "# Later").unwrap();
    store.delete(&later.name, later.revision.as_str()).unwrap();
    drop(store);
    let mut store = Store::open(directory.path()).unwrap();
    assert_eq!(store.trash().unwrap().len(), 1);
    assert_eq!(store.trash().unwrap()[0].name, later.name);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    for _ in 0..20 {
        store.process_recovery_cleanup(now, 8).unwrap();
    }
    assert_eq!(
        std::fs::read_dir(directory.path().join(".trash"))
            .unwrap()
            .count(),
        1
    );
    let saved = store.trash().unwrap().remove(0);
    assert_eq!(
        store.restore_trash(&saved.id, None).unwrap().body,
        "# Later"
    );
}

#[test]
fn trash_pages_have_stable_cursors_and_keep_the_oldest_deleted_notes_reachable() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = tungsten::store::Store::open(directory.path()).unwrap();
    for index in 0..125 {
        let note = store
            .create(
                Some(format!("Entry-{index}.md")),
                &format!("# Entry {index}"),
            )
            .unwrap();
        store.delete(&note.name, note.revision.as_str()).unwrap();
    }
    let mut cursor = None;
    let mut ids = std::collections::HashSet::new();
    let mut sizes = Vec::new();
    loop {
        let page = store.trash_page(cursor.as_deref()).unwrap();
        sizes.push(page.entries.len());
        for entry in &page.entries {
            assert!(ids.insert(entry.id.clone()));
            assert!(store.saved_note(&entry.id, true).is_ok());
        }
        cursor = page.next;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(sizes, [50, 50, 25]);
    assert_eq!(ids.len(), 125);
    for cursor in [
        "bad",
        "-1:invalid",
        "9999999999999999999999999:invalid",
        "1:../escape",
    ] {
        assert!(store.trash_page(Some(cursor)).is_err());
    }
}
