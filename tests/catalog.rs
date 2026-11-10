use tungsten::{domain::NoteName, store::Store};

#[test]
fn current_cursors_require_the_complete_shape() {
    use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(directory.path()).unwrap();
    store.create(Some("one.md".into()), "# First").unwrap();
    store.create(Some("two.md".into()), "# Second").unwrap();
    let cursor = store.search("", None, 1, false).unwrap().next.unwrap();
    let mut value: serde_json::Value =
        serde_json::from_slice(&BASE64_URL_SAFE_NO_PAD.decode(cursor).unwrap()).unwrap();
    value.as_object_mut().unwrap().remove("exact").unwrap();
    let incomplete = BASE64_URL_SAFE_NO_PAD.encode(serde_json::to_vec(&value).unwrap());
    assert!(store.search("", Some(&incomplete), 1, false).is_err());
}

#[test]
fn current_recovery_records_require_a_title() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(directory.path()).unwrap();
    let note = store.create(Some("keep.md".into()), "# Kept").unwrap();
    store.delete(&note.name, note.revision.as_str()).unwrap();
    assert_eq!(store.trash().unwrap()[0].title, "Kept");
    let database = rusqlite::Connection::open(directory.path().join("notes.sqlite")).unwrap();
    database
        .execute(
            "UPDATE trash_visible SET summary=json_remove(summary,'$.title')",
            [],
        )
        .unwrap();
    assert!(store.trash().is_err());
    assert!(store.trash_page(None).is_err());
}

#[test]
fn unsupported_database_formats_fail_without_converting_saved_records() {
    for format in [0, 1, 2, 4] {
        let directory = tempfile::tempdir().unwrap();
        let mut store = Store::open(directory.path()).unwrap();
        let note = store
            .create(Some("keep.md".into()), "# Keep\n\nSaved writing.")
            .unwrap();
        drop(store);
        let database = rusqlite::Connection::open(directory.path().join("notes.sqlite")).unwrap();
        database
            .pragma_update(None, "user_version", format)
            .unwrap();
        assert!(Store::open(directory.path()).is_err(), "format {format}");
        assert_eq!(
            database
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            format
        );
        assert_eq!(
            database
                .query_row(
                    "SELECT body FROM documents WHERE name=?",
                    [note.name.as_str()],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            note.body
        );
    }
}

#[test]
fn current_indexes_preserve_source_records_across_restarts() {
    let directory = tempfile::tempdir().unwrap();
    let vault = directory.path().join("vault");
    let mut store = Store::open(&vault).unwrap();
    let first = store
        .create(Some("one.md".into()), "# first\n\nabc bcd 日本語")
        .unwrap();
    let second = store
        .create(Some("two.md".into()), "# second\n\nabcd 日本語")
        .unwrap();
    store
        .pin(&first.name, true, first.revision.as_str())
        .unwrap();
    let workspace = store.create_workspace("Kept").unwrap();
    let workspace = store
        .workspace_member(workspace.id, &first.name, true)
        .unwrap();
    let before = directory.path().join("before");
    store.backup_to(&before).unwrap();
    drop(store);
    let store = Store::open(&vault).unwrap();
    assert_eq!(store.read(&first.name).unwrap().body, first.body);
    assert_eq!(store.list("abcd").unwrap()[0].name, second.name);
    assert_eq!(store.list("-abcd").unwrap()[0].name, first.name);
    assert_eq!(
        store.workspace(workspace.id).unwrap().revision,
        workspace.revision
    );
    let after = directory.path().join("after");
    store.backup_to(&after).unwrap();
    assert_eq!(
        std::fs::read(before.join("metadata.jsonl")).unwrap(),
        std::fs::read(after.join("metadata.jsonl")).unwrap()
    );
    let database = rusqlite::Connection::open(vault.join("notes.sqlite")).unwrap();
    assert_eq!(
        database
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        3
    );
}

#[test]
fn substring_search_matches_literal_terms_when_windows_repeat_or_change_order() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(directory.path()).unwrap();
    let titles = [
        "abcd",
        "abc bcd",
        "bcd abc",
        "abcabc",
        "bcabca",
        "日本語の本文",
        "日本 語の の本 本文",
        "x\"y[z]",
        "café",
        "cafe\u{301}",
        "a😀日b",
        "a😀 😀日 日b",
        "abc\0def",
        "after\0needle",
    ];
    let mut fixtures = Vec::new();
    for (index, title) in titles.iter().enumerate() {
        let name = format!("entry-{index:02}.md");
        let body = format!("# {title}\n\n{title}\n#work");
        store.create(Some(name.clone()), &body).unwrap();
        fixtures.push((name, title.to_lowercase(), body.to_lowercase()));
    }
    for titles_only in [false, true] {
        for query in [
            "abcd",
            "-abcd",
            "-bcd",
            "-abcdef",
            "abcabc",
            "-abcabc",
            "abc -abcd",
            "bcabca",
            "日本語の本文",
            "-日本語の本文",
            "日 -日本語の本文",
            "x\"y[z]",
            "-x\"y[z]",
            "café",
            "cafe\u{301}",
            "a😀日b",
            "-a😀日b",
            "abc\0def",
            "needle",
            "\0",
            "\0d",
            "c\0d",
            "-c\0d",
        ] {
            let mut expected = fixtures
                .iter()
                .rev()
                .filter(|(name, title, body)| {
                    let text = if titles_only {
                        title.clone()
                    } else {
                        format!("{name}\n{body}")
                    };
                    query.split_whitespace().all(|term| {
                        term.strip_prefix('-')
                            .map_or_else(|| text.contains(term), |term| !text.contains(term))
                    })
                })
                .collect::<Vec<_>>();
            if !query.starts_with('-') && !query.contains(' ') {
                expected.sort_by_key(|(_, title, _)| title != query);
            }
            let expected = expected
                .into_iter()
                .map(|(name, _, _)| name.clone())
                .collect::<Vec<_>>();
            let mut actual = Vec::new();
            let mut cursor = None;
            loop {
                let page = store
                    .search(query, cursor.as_deref(), 3, titles_only)
                    .unwrap();
                actual.extend(
                    page.notes
                        .into_iter()
                        .map(|note| note.name.as_str().to_owned()),
                );
                cursor = page.next;
                if cursor.is_none() {
                    break;
                }
                assert!(actual.len() <= fixtures.len());
            }
            assert_eq!(actual, expected, "{query:?} titles={titles_only}");
        }
    }
}

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
fn catalog_tracks_writes_deletes_restores_and_rejects_external_files() {
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
    std::fs::write(directory.path().join("outside.md"), "# External").unwrap();
    assert!(Store::open(directory.path()).is_err());
    std::fs::remove_file(directory.path().join("outside.md")).unwrap();
    let store = Store::open(directory.path()).unwrap();
    assert_eq!(store.list("replacement").unwrap().len(), 1);
    assert_eq!(
        store
            .read(&NoteName::parse("a.md".into()).unwrap())
            .unwrap()
            .body,
        "# Second\n\nreplacement"
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
    use tungsten::{domain::Revision, store::SavedNote};
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(directory.path()).unwrap();
    store.create(Some("a.md".into()), "# Current").unwrap();
    let database = rusqlite::Connection::open(directory.path().join("notes.sqlite")).unwrap();
    let captured = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    for index in 0..600 {
        let id = format!("{index:032x}");
        let body = format!("# Version {index}\n");
        let name = if index % 2 == 0 { "a.md" } else { "foreign.md" };
        let revision = Revision::of(body.as_bytes());
        let timestamp = captured - index;
        let summary = SavedNote {
            id: id.clone(),
            name: NoteName::parse(name.into()).unwrap(),
            modified: captured as u128,
            revision: revision.clone(),
            bytes: body.len(),
            captured: timestamp as u128,
            title: format!("Version {index}"),
        };
        database
            .execute(
                "INSERT INTO snapshots VALUES(0,?,?,?,?,?,?)",
                rusqlite::params![id, name, body, revision.as_str(), captured, timestamp],
            )
            .unwrap();
        database
            .execute(
                "INSERT INTO records VALUES(0,?,?,?,?,?,?)",
                rusqlite::params![
                    id,
                    name,
                    timestamp,
                    serde_json::to_string(&summary).unwrap(),
                    revision.as_str(),
                    index + 1
                ],
            )
            .unwrap();
    }
    database
        .execute("UPDATE state SET value=600 WHERE key='record-sequence'", [])
        .unwrap();
    database
        .execute(
            "UPDATE snapshots SET body='corrupt unrelated body' WHERE id=?",
            [format!("{:032x}", 1)],
        )
        .unwrap();
    let name = NoteName::parse("a.md".into()).unwrap();
    let history = store.history(&name).unwrap();
    assert_eq!(history.len(), 50);
    assert_eq!(history[0].id, format!("{:032x}", 0));
    assert_eq!(
        store.saved_note(&history[0].id, false).unwrap().body,
        "# Version 0\n"
    );
    assert!(store.saved_note(&format!("{:032x}", 1), false).is_err());
    assert_eq!(
        store
            .collect_unused_attachments(std::time::SystemTime::now())
            .unwrap(),
        0
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
    let database = rusqlite::Connection::open(directory.path().join("notes.sqlite")).unwrap();
    assert_eq!(
        database
            .query_row("SELECT count(*) FROM snapshots WHERE trash=1", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
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
