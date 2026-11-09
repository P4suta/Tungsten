use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

async fn call(
    app: &Router,
    method: &str,
    uri: &str,
    input: Value,
    revision: Option<&str>,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "localhost")
        .header("content-type", "application/json");
    if let Some(revision) = revision {
        request = request.header("if-match", revision);
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::from(input.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn previews_markdown_without_active_html_or_remote_images() {
    let directory = tempfile::tempdir().unwrap();
    let app = tungsten::api::app(directory.path()).unwrap();
    let (status, rendered) = call(&app, "POST", "/api/preview", json!({"name":"Work/Today.md", "body":"# Heading\n\n**strong** ~~gone~~\n\n- [ ] Task\n\n| A | B |\n| - | - |\n| x | y |\n\n[[Other|Alias]] #work\n\n`#ignored`\n\n<script>alert(1)</script>\n\n[bad](javascript:alert%281%29)\n\n![track](https://evil.test/pixel.png)"}), None).await;
    assert_eq!(status, StatusCode::OK);
    let html = rendered["html"].as_str().unwrap();
    assert!(html.contains("<strong>strong</strong>"));
    assert!(html.contains("<table>"));
    assert!(html.contains("<del>gone</del>"));
    assert!(!html.contains("<script>"));
    assert!(!html.contains("href=\"javascript:"));
    assert!(!html.contains("src=\"https://evil.test"));
    assert_eq!(rendered["tags"], json!(["work"]));
    assert_eq!(rendered["links"], json!(["Work/Other.md"]));
    assert_eq!(rendered["outline"][0]["text"], "Heading");
    assert_eq!(rendered["tasks"][0]["checked"], false);
    assert_eq!(rendered["tasks"][0]["label"], "Task: Task");
}

#[tokio::test]
async fn folders_pins_and_backlinks_survive_moves_and_restarts() {
    let directory = tempfile::tempdir().unwrap();
    let app = tungsten::api::app(directory.path()).unwrap();
    let (status, note) = call(
        &app,
        "POST",
        "/api/notes",
        json!({"name":"Work/Plan.md", "body":"# Plan\n#work"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let revision = note["revision"].as_str().unwrap();
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/pins",
            json!({"name":"Work/Plan.md", "pinned":true}),
            Some(revision)
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    call(
        &app,
        "POST",
        "/api/notes",
        json!({"name":"Work/Index.md", "body":"[[Plan]]"}),
        None,
    )
    .await;
    let (_, notes) = call(&app, "GET", "/api/notes?q=%23work", Value::Null, None).await;
    assert_eq!(notes.as_array().unwrap().len(), 1);
    assert_eq!(notes[0]["pinned"], true);
    assert_eq!(notes[0]["tags"], json!(["work"]));
    let (_, backlinks) = call(
        &app,
        "GET",
        "/api/backlinks?name=Work%2FPlan.md",
        Value::Null,
        None,
    )
    .await;
    assert_eq!(backlinks[0]["name"], "Work/Index.md");
    let (status, _) = call(
        &app,
        "PATCH",
        "/api/notes/Work%2FPlan.md",
        json!({"name":"Archive/Plan.md"}),
        Some(revision),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        std::fs::read_to_string(directory.path().join("Archive/Plan.md")).unwrap(),
        "# Plan\n#work"
    );
    drop(app);
    let app = tungsten::api::app(directory.path()).unwrap();
    let (_, notes) = call(&app, "GET", "/api/notes?q=Archive", Value::Null, None).await;
    assert_eq!(notes[0]["pinned"], true);
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), directory.path().join("Escape")).unwrap();
    assert_ne!(
        call(
            &app,
            "POST",
            "/api/notes",
            json!({"name":"Escape/Leak.md", "body":"no"}),
            None
        )
        .await
        .0,
        StatusCode::CREATED
    );
    assert!(!outside.path().join("Leak.md").exists());
}

#[tokio::test]
async fn history_and_trash_restore_without_losing_conflicting_notes() {
    let directory = tempfile::tempdir().unwrap();
    let app = tungsten::api::app(directory.path()).unwrap();
    let (_, first) = call(
        &app,
        "POST",
        "/api/notes",
        json!({"name":"History.md", "body":"before"}),
        None,
    )
    .await;
    let (_, second) = call(
        &app,
        "PUT",
        "/api/notes/History.md",
        json!({"body":"after"}),
        first["revision"].as_str(),
    )
    .await;
    let (_, history) = call(
        &app,
        "GET",
        "/api/history?name=History.md",
        Value::Null,
        None,
    )
    .await;
    let id = history[0]["id"].as_str().unwrap();
    let path = format!("/api/history/{id}/restore");
    assert_eq!(
        call(
            &app,
            "POST",
            &path,
            json!({"name":"History.md"}),
            first["revision"].as_str()
        )
        .await
        .0,
        StatusCode::PRECONDITION_FAILED
    );
    let (status, restored) = call(
        &app,
        "POST",
        &path,
        json!({"name":"History.md"}),
        second["revision"].as_str(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(restored["body"], "before");
    assert_eq!(
        call(
            &app,
            "DELETE",
            "/api/notes/History.md",
            Value::Null,
            restored["revision"].as_str()
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let (_, trash) = call(&app, "GET", "/api/trash", Value::Null, None).await;
    let id = trash[0]["id"].as_str().unwrap();
    call(
        &app,
        "POST",
        "/api/notes",
        json!({"name":"History.md", "body":"keep"}),
        None,
    )
    .await;
    let path = format!("/api/trash/{id}/restore");
    assert_eq!(
        call(&app, "POST", &path, json!({}), None).await.0,
        StatusCode::CONFLICT
    );
    let (status, restored) = call(
        &app,
        "POST",
        &path,
        json!({"name":"Recovered/History.md"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(restored["body"], "before");
    let (_, saved) = call(&app, "GET", "/api/notes/History.md", Value::Null, None).await;
    assert_eq!(saved["body"], "keep");
    assert_eq!(
        call(&app, "GET", "/api/trash", Value::Null, None).await.1,
        json!([])
    );
}

#[tokio::test]
async fn removed_import_routes_leave_existing_notes_and_attachment_downloads_intact() {
    let directory = tempfile::tempdir().unwrap();
    let app = tungsten::api::app(directory.path()).unwrap();
    call(
        &app,
        "POST",
        "/api/notes",
        json!({"name":"Keep.md", "body":"saved"}),
        None,
    )
    .await;
    assert_eq!(
        call(&app, "POST", "/api/import", json!({"notes":[]}), None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, "GET", "/api/export?name=Keep.md", Value::Null, None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let (status, attachment) = call(
        &app,
        "POST",
        "/api/attachments",
        json!({"name":"hello.txt", "data":"aGVsbG8="}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let url = attachment["url"].as_str().unwrap();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(url)
                .header("host", "localhost")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers()["content-disposition"]
            .to_str()
            .unwrap()
            .starts_with("attachment")
    );
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        "hello"
    );
    assert_eq!(
        std::fs::read_to_string(directory.path().join("Keep.md")).unwrap(),
        "saved"
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/import",
            json!({"notes":[{"name":"../Escape.md", "body":"bad"}]}),
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn recreating_a_name_does_not_inherit_a_deleted_pin_and_history_tracks_repeated_text() {
    let directory = tempfile::tempdir().unwrap();
    let app = tungsten::api::app(directory.path()).unwrap();
    let (_, note) = call(
        &app,
        "POST",
        "/api/notes",
        json!({"name":"Repeated.md", "body":"first"}),
        None,
    )
    .await;
    call(
        &app,
        "POST",
        "/api/pins",
        json!({"name":"Repeated.md", "pinned":true}),
        note["revision"].as_str(),
    )
    .await;
    call(
        &app,
        "DELETE",
        "/api/notes/Repeated.md",
        Value::Null,
        note["revision"].as_str(),
    )
    .await;
    let (_, note) = call(
        &app,
        "POST",
        "/api/notes",
        json!({"name":"Repeated.md", "body":"first"}),
        None,
    )
    .await;
    let (_, summaries) = call(&app, "GET", "/api/notes", Value::Null, None).await;
    assert_eq!(summaries[0]["pinned"], false);
    let (_, second) = call(
        &app,
        "PUT",
        "/api/notes/Repeated.md",
        json!({"body":"second"}),
        note["revision"].as_str(),
    )
    .await;
    let (_, first) = call(
        &app,
        "PUT",
        "/api/notes/Repeated.md",
        json!({"body":"first"}),
        second["revision"].as_str(),
    )
    .await;
    call(
        &app,
        "PUT",
        "/api/notes/Repeated.md",
        json!({"body":"third"}),
        first["revision"].as_str(),
    )
    .await;
    let (_, history) = call(
        &app,
        "GET",
        "/api/history?name=Repeated.md",
        Value::Null,
        None,
    )
    .await;
    assert_eq!(history.as_array().unwrap().len(), 1);
}
