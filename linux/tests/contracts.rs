use tungsten_linux::{
    Error, NoteName,
    client::Client,
    drafts::{Draft, DraftStore},
    editor::Editor,
    types::ServerAddress,
};

async fn server() -> (tempfile::TempDir, Client, tokio::task::JoinHandle<()>) {
    let directory = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let router = tungsten::api::app_with_origin(
        directory.path(),
        tungsten::api::ServerOrigin::development(&origin).unwrap(),
    )
    .unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (
        directory,
        Client::new(ServerAddress::parse(&origin, true).unwrap()).unwrap(),
        task,
    )
}

#[test]
fn origins_reject_paths_credentials_insecure_remote_hosts_and_non_origins() {
    for value in [
        "http://example.com",
        "https://a/b",
        "https://u:p@a",
        "https://a?x",
        "https://a#x",
        "file:///a",
        "https://a:0",
    ] {
        assert!(ServerAddress::parse(value, true).is_err(), "{value}");
    }
    assert!(ServerAddress::parse("http://localhost:8080", false).is_err());
    assert_eq!(
        ServerAddress::parse("https://EXAMPLE.com:443/", false)
            .unwrap()
            .as_str(),
        "https://example.com"
    );
    assert!(ServerAddress::parse("http://[::1]:8080", true).is_ok());
}

#[tokio::test]
async fn interrupted_writes_and_concurrent_edits_recover_against_the_real_server() {
    let (_server, client, task) = server().await;
    let local = tempfile::tempdir().unwrap();
    let store = DraftStore::at(local.path().to_owned());
    let mut editor = Editor::create(client.address().clone(), 1);
    editor.edit("# First".into(), &store).unwrap();
    let (ticket, outgoing) = editor.begin(&store).unwrap().unwrap();
    editor.edit("# First\n\n追加".into(), &store).unwrap();
    let saved = client.save(&outgoing).await.unwrap();
    editor.complete(ticket, Ok(saved), &store).unwrap();
    assert_eq!(editor.draft().body, "# First\n\n追加");
    assert!(!editor.state().is_saved());
    let (_ticket, outgoing) = editor.begin(&store).unwrap().unwrap();
    let committed = client.save(&outgoing).await.unwrap();
    let draft = store
        .recover(client.address())
        .unwrap()
        .drafts
        .pop()
        .unwrap();
    let mut recovered = Editor::recover(draft, 2);
    assert!(recovered.begin(&store).unwrap().is_none());
    recovered
        .reconcile(client.read(&committed.name).await, &store)
        .unwrap();
    let (ticket, outgoing) = recovered.begin(&store).unwrap().unwrap();
    recovered
        .complete(ticket, client.save(&outgoing).await, &store)
        .unwrap();
    assert!(recovered.state().is_saved());
    assert!(store.recover(client.address()).unwrap().drafts.is_empty());
    task.abort();
}

#[tokio::test]
async fn revisions_conflicts_remote_deletion_workspaces_history_trash_and_original_bytes() {
    let (_server, client, task) = server().await;
    let name = NoteName::parse("日本語/Note.md".into()).unwrap();
    let mut draft = Draft {
        format: 1,
        server: client.address().clone(),
        name: name.clone(),
        body: "# Original".into(),
        revision: None,
        pending: None,
    };
    let first = client.save(&draft).await.unwrap();
    assert_eq!(client.read(&name).await.unwrap(), first);
    draft.revision = Some(first.revision.clone());
    draft.body = "# Changed".into();
    let changed = client.save(&draft).await.unwrap();
    assert!(matches!(client.save(&draft).await, Err(Error::Conflict)));
    assert_eq!(client.notes("Changed", None).await.unwrap().notes.len(), 1);
    let workspace = client.create_workspace("Writing").await.unwrap();
    client.membership(&workspace.id, &name, true).await.unwrap();
    assert_eq!(
        client
            .workspace_notes(&workspace.id, None)
            .await
            .unwrap()
            .notes[0]
            .name,
        name
    );
    assert_eq!(client.workspaces(None).await.unwrap().workspaces.len(), 1);
    let history = client.history(&name).await.unwrap();
    assert!(history.iter().any(|record| record.name == name));
    assert_eq!(client.saved(&history[0], false).await.unwrap().name, name);
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), [0, 255, 11, 22]).unwrap();
    let attachment = client.upload(file.path(), "original.bin").await.unwrap();
    let downloaded = client
        .download(&attachment, file.path().parent().unwrap())
        .await
        .unwrap();
    assert_eq!(std::fs::read(&downloaded).unwrap(), [0, 255, 11, 22]);
    client.delete(&changed).await.unwrap();
    assert!(matches!(client.save(&draft).await, Err(Error::Missing)));
    let trash = client.trash(None).await.unwrap();
    assert_eq!(trash.entries.len(), 1);
    assert_eq!(
        client
            .restore_trash(&trash.entries[0], &name)
            .await
            .unwrap()
            .body,
        changed.body
    );
    task.abort();
}

#[test]
fn corrupt_drafts_are_kept_while_valid_server_scoped_drafts_recover() {
    let directory = tempfile::tempdir().unwrap();
    let store = DraftStore::at(directory.path().to_owned());
    let first = ServerAddress::parse("https://first.example", false).unwrap();
    let other = ServerAddress::parse("https://other.example", false).unwrap();
    let mut editor = Editor::create(first.clone(), 1);
    editor.edit("# Recover".into(), &store).unwrap();
    let folder = std::fs::read_dir(directory.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let damaged = folder.join("damaged.json");
    std::fs::write(&damaged, b"{").unwrap();
    let recovery = store.recover(&first).unwrap();
    assert_eq!(recovery.drafts.len(), 1);
    assert_eq!(recovery.damaged.as_slice(), std::slice::from_ref(&damaged));
    assert_eq!(std::fs::read(damaged).unwrap(), b"{");
    assert!(store.recover(&other).unwrap().drafts.is_empty());
    std::fs::remove_dir_all(&folder).unwrap();
    std::fs::write(folder, b"blocked").unwrap();
    assert!(editor.edit("Keep in memory".into(), &store).is_err());
    assert!(!editor.state().may_leave());
    assert_eq!(editor.draft().body, "Keep in memory");
}
