use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

async fn request(
    app: &Router,
    method: &str,
    url: &str,
    body: Value,
    revision: Option<&str>,
) -> (StatusCode, Value, Option<String>) {
    let mut request = Request::builder()
        .method(method)
        .uri(url)
        .header("content-type", "application/json")
        .header("host", "localhost")
        .header("origin", "http://localhost");
    if let Some(revision) = revision {
        request = request.header("if-match", revision);
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let revision = response
        .headers()
        .get("etag")
        .map(|value| value.to_str().unwrap().to_owned());
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value, revision)
}

#[tokio::test]
async fn workspaces_keep_independent_membership_and_reject_stale_metadata_changes() {
    let directory = tempfile::tempdir().unwrap();
    let app = tungsten::api::app(directory.path()).unwrap();
    let (status, group, revision) = request(
        &app,
        "POST",
        "/api/workspaces",
        json!({"title":"Project A"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = group["id"].as_str().unwrap();
    let path = format!("/api/workspaces/{id}");
    let revision = revision.unwrap();
    for name in ["One.md", "Two.md"] {
        assert_eq!(
            request(
                &app,
                "POST",
                "/api/notes",
                json!({"name":name,"body":format!("# {name}\n")}),
                None
            )
            .await
            .0,
            StatusCode::CREATED
        );
        assert_eq!(
            request(
                &app,
                "PUT",
                &format!("{path}/notes/{name}"),
                json!({}),
                None
            )
            .await
            .0,
            StatusCode::OK
        );
    }
    let (_, members, _) = request(&app, "GET", &format!("{path}/notes"), Value::Null, None).await;
    assert_eq!(members["notes"].as_array().unwrap().len(), 2);
    assert_eq!(
        request(
            &app,
            "PUT",
            &path,
            json!({"title":"Stale rename"}),
            Some(&revision)
        )
        .await
        .0,
        StatusCode::PRECONDITION_FAILED
    );
    let (_, current, revision) = request(&app, "GET", &path, Value::Null, None).await;
    assert_eq!(current["title"], "Project A");
    assert_eq!(
        request(&app, "DELETE", &path, Value::Null, revision.as_deref())
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    for name in ["One.md", "Two.md"] {
        assert_eq!(
            request(
                &app,
                "GET",
                &format!("/api/notes/{name}"),
                Value::Null,
                None
            )
            .await
            .0,
            StatusCode::OK
        );
    }
}

#[test]
fn membership_survives_storage_renames_and_never_attaches_to_recreated_notes() {
    use tungsten::{domain::NoteName, store::Store};
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(directory.path()).unwrap();
    let first = store.create_workspace("First").unwrap();
    let second = store.create_workspace("Second").unwrap();
    let note = store
        .create(Some("Original.md".into()), "# Original")
        .unwrap();
    for group in [&first, &second] {
        store.workspace_member(group.id, &note.name, true).unwrap();
    }
    let before = store.workspace_notes(first.id, None, 50).unwrap();
    let updated = store
        .update(&note.name, "# Changed", note.revision.as_str())
        .unwrap();
    let changed = store.workspace_notes(first.id, None, 50).unwrap();
    assert_ne!(before.revision, changed.revision);
    assert_eq!(changed.notes[0].title, "Changed");
    let renamed = store
        .rename(
            &note.name,
            &NoteName::parse("Renamed.md".into()).unwrap(),
            updated.revision.as_str(),
        )
        .unwrap();
    for group in [&first, &second] {
        let page = store.workspace_notes(group.id, None, 50).unwrap();
        assert_eq!(page.notes[0].name, renamed.name);
    }
    store
        .delete(&renamed.name, renamed.revision.as_str())
        .unwrap();
    store
        .create(Some("Renamed.md".into()), "# A different note")
        .unwrap();
    for group in [&first, &second] {
        assert!(
            store
                .workspace_notes(group.id, None, 50)
                .unwrap()
                .notes
                .is_empty()
        );
    }
    drop(store);
    let store = Store::open(directory.path()).unwrap();
    assert_eq!(store.workspaces(None, 50).unwrap().workspaces.len(), 2);
}

#[test]
fn workspace_pages_are_bounded_and_mutations_are_idempotent() {
    use tungsten::store::Store;
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(directory.path()).unwrap();
    for index in 0..55 {
        store
            .create_workspace(&format!("Workspace {index}"))
            .unwrap();
    }
    let first = store.workspaces(None, 50).unwrap();
    assert_eq!(first.workspaces.len(), 50);
    let second = store.workspaces(first.next.as_deref(), 50).unwrap();
    assert_eq!(second.workspaces.len(), 5);
    assert!(second.next.is_none());
    let id = first.workspaces[0].id;
    for index in 0..55 {
        let note = store
            .create(Some(format!("Note-{index}.md")), &format!("# Note {index}"))
            .unwrap();
        let changed = store.workspace_member(id, &note.name, true).unwrap();
        let repeated = store.workspace_member(id, &note.name, true).unwrap();
        assert_eq!(changed.revision, repeated.revision);
    }
    let first = store.workspace_notes(id, None, 50).unwrap();
    assert_eq!(first.notes.len(), 50);
    let second = store
        .workspace_notes(id, first.next.as_deref(), 50)
        .unwrap();
    assert_eq!(second.notes.len(), 5);
    assert!(second.next.is_none());
    assert_ne!(first.revision, second.revision);
    assert!(store.workspaces(None, 51).is_err());
    assert!(store.workspace_notes(id, Some("-1"), 50).is_err());
}

#[test]
fn incomplete_workspace_schemas_fail_without_replacing_saved_notes() {
    use tungsten::store::Store;
    for change in [
        "DROP INDEX workspace_note_order",
        "DROP INDEX workspace_note_sources",
        "DROP TRIGGER workspace_note_title",
        "DROP TRIGGER workspace_member_insert",
        "DROP TRIGGER workspace_member_delete",
        "DROP TRIGGER workspace_member_update",
        "DROP TRIGGER workspace_insert",
        "DROP TRIGGER workspace_update",
        "DROP TRIGGER workspace_delete",
        "DELETE FROM workspace_state",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut store = Store::open(directory.path()).unwrap();
        store
            .create(Some("Preserved.md".into()), "# Preserved")
            .unwrap();
        drop(store);
        let database = rusqlite::Connection::open(directory.path().join("notes.sqlite")).unwrap();
        database.execute_batch(change).unwrap();
        drop(database);
        assert!(Store::open(directory.path()).is_err(), "{change}");
        let database = rusqlite::Connection::open(directory.path().join("notes.sqlite")).unwrap();
        assert_eq!(
            database
                .query_row(
                    "SELECT body FROM documents WHERE name='Preserved.md'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "# Preserved"
        );
    }
}

#[test]
fn revision_exhaustion_rolls_back_note_deletion_and_membership() {
    use tungsten::store::Store;
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(directory.path()).unwrap();
    let workspace = store.create_workspace("Keep").unwrap();
    let note = store.create(Some("Keep.md".into()), "# Keep").unwrap();
    store
        .workspace_member(workspace.id, &note.name, true)
        .unwrap();
    let database = rusqlite::Connection::open(directory.path().join("notes.sqlite")).unwrap();
    database
        .execute("UPDATE workspaces SET revision=?", [i64::MAX])
        .unwrap();
    assert!(store.delete(&note.name, note.revision.as_str()).is_err());
    assert_eq!(store.read(&note.name).unwrap().body, note.body);
    assert_eq!(
        store
            .workspace_notes(workspace.id, None, 50)
            .unwrap()
            .notes
            .len(),
        1
    );
    assert!(store.trash().unwrap().is_empty());
}

#[tokio::test]
async fn conditional_reads_and_concurrent_membership_preserve_independent_changes() {
    let directory = tempfile::tempdir().unwrap();
    let app = tungsten::api::app(directory.path()).unwrap();
    let (_, group, _) = request(
        &app,
        "POST",
        "/api/workspaces",
        json!({"title":"Shared"}),
        None,
    )
    .await;
    let path = format!("/api/workspaces/{}", group["id"].as_str().unwrap());
    let (_, _, revision) = request(&app, "GET", "/api/workspaces", Value::Null, None).await;
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/workspaces")
                .header("host", "localhost")
                .header("if-none-match", revision.unwrap())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
    assert!(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .is_empty()
    );
    for name in ["Left.md", "Right.md"] {
        request(
            &app,
            "POST",
            "/api/notes",
            json!({"name":name,"body":"# Shared note"}),
            None,
        )
        .await;
    }
    let left = format!("{path}/notes/Left.md");
    let right = format!("{path}/notes/Right.md");
    let (left, right) = tokio::join!(
        request(&app, "PUT", &left, json!({}), None),
        request(&app, "PUT", &right, json!({}), None)
    );
    assert_eq!(left.0, StatusCode::OK);
    assert_eq!(right.0, StatusCode::OK);
    let (_, page, _) = request(&app, "GET", &format!("{path}/notes"), Value::Null, None).await;
    assert_eq!(page["notes"].as_array().unwrap().len(), 2);
    let (_, _, revision) = request(&app, "GET", &path, Value::Null, None).await;
    let (left, right) = tokio::join!(
        request(
            &app,
            "PUT",
            &path,
            json!({"title":"Left"}),
            revision.as_deref()
        ),
        request(
            &app,
            "PUT",
            &path,
            json!({"title":"Right"}),
            revision.as_deref()
        )
    );
    assert_eq!(
        [left.0, right.0]
            .iter()
            .filter(|status| **status == StatusCode::OK)
            .count(),
        1
    );
    assert_eq!(
        [left.0, right.0]
            .iter()
            .filter(|status| **status == StatusCode::PRECONDITION_FAILED)
            .count(),
        1
    );
}

#[tokio::test]
async fn invalid_workspace_inputs_and_missing_preconditions_never_change_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let app = tungsten::api::app(directory.path()).unwrap();
    for title in [
        String::new(),
        "\nHidden".into(),
        "A\u{2028}B".into(),
        "a".repeat(121),
    ] {
        assert_eq!(
            request(
                &app,
                "POST",
                "/api/workspaces",
                json!({"title":title}),
                None
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    let (_, group, revision) = request(
        &app,
        "POST",
        "/api/workspaces",
        json!({"title":"Valid"}),
        None,
    )
    .await;
    let path = format!("/api/workspaces/{}", group["id"].as_str().unwrap());
    assert_eq!(
        request(
            &app,
            "POST",
            "/api/workspaces",
            json!({"title":"Valid"}),
            None
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        request(&app, "PUT", &path, json!({"title":"Changed"}), None)
            .await
            .0,
        StatusCode::PRECONDITION_REQUIRED
    );
    for id in ["0", "01", "-1", "9223372036854775808"] {
        assert_eq!(
            request(
                &app,
                "GET",
                &format!("/api/workspaces/{id}"),
                Value::Null,
                None
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    let (_, value, unchanged) = request(&app, "GET", &path, Value::Null, None).await;
    assert_eq!(value["title"], "Valid");
    assert_eq!(unchanged, revision);
}
