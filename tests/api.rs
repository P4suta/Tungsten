use std::{path::Path, sync::Arc};

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

#[test]
fn router_construction_does_not_require_an_active_async_runtime() {
    let directory = tempfile::tempdir().unwrap();
    assert!(tungsten::api::app(directory.path()).is_ok());
}

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
        .header("host", "notes.tail.ts.net")
        .header("origin", "https://notes.tail.ts.net");
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
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, value, revision)
}

#[tokio::test]
async fn conditional_note_reads_preserve_private_headers_and_return_no_unchanged_body() {
    let directory = tempfile::tempdir().unwrap();
    let app = tungsten::api::app(directory.path()).unwrap();
    let (_, _, revision) = request(
        &app,
        "POST",
        "/api/notes",
        json!({"name":"Shared.md","body":"# Shared\n\nOriginal"}),
        None,
    )
    .await;
    let revision = revision.unwrap();
    for tag in [
        revision.clone(),
        format!("W/{revision}"),
        format!("\"other\", W/{revision}"),
        "*".to_owned(),
    ] {
        for method in ["GET", "HEAD"] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri("/api/notes/Shared.md")
                        .header("host", "localhost")
                        .header("if-none-match", &tag)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
            assert_eq!(response.headers()["etag"], revision);
            assert!(
                response.headers()["cache-control"]
                    .to_str()
                    .unwrap()
                    .contains("no-store")
            );
            assert!(
                response
                    .into_body()
                    .collect()
                    .await
                    .unwrap()
                    .to_bytes()
                    .is_empty()
            );
        }
    }
    assert_eq!(
        request(
            &app,
            "PUT",
            "/api/notes/Shared.md",
            json!({"body":"Changed"}),
            Some(&revision)
        )
        .await
        .0,
        StatusCode::OK
    );
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/notes/Shared.md")
                .header("host", "localhost")
                .header("if-none-match", revision)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let value: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(value["body"], "Changed");
}

#[tokio::test]
async fn browser_assets_revalidate_and_compress_without_caching_private_pages() {
    let directory = tempfile::tempdir().unwrap();
    let app = tungsten::api::app(directory.path()).unwrap();
    let fetch = |path: String, encoding: &str, etag: Option<&str>| {
        let mut request = Request::builder()
            .uri(path)
            .header("host", "localhost")
            .header("accept-encoding", encoding);
        if let Some(etag) = etag {
            request = request.header("if-none-match", etag);
        }
        app.clone().oneshot(request.body(Body::empty()).unwrap())
    };
    let plain = fetch("/assets/app.js".into(), "identity", None)
        .await
        .unwrap();
    assert_eq!(plain.status(), StatusCode::OK);
    assert_eq!(
        plain.headers()["cache-control"],
        "public, max-age=0, must-revalidate"
    );
    let etag = plain.headers()["etag"].to_str().unwrap().to_owned();
    let bytes = plain.into_body().collect().await.unwrap().to_bytes();
    let text = std::str::from_utf8(&bytes).unwrap();
    let chunk = text
        .split('"')
        .find(|part| part.starts_with("./chunks/") && part.ends_with(".js"))
        .unwrap();
    let chunk = fetch(
        format!("/assets/{}", chunk.trim_start_matches("./")),
        "identity",
        None,
    )
    .await
    .unwrap();
    assert_eq!(chunk.status(), StatusCode::OK);
    assert_eq!(
        chunk.headers()["cache-control"],
        "public, max-age=31536000, immutable"
    );
    let compressed = fetch("/assets/app.js".into(), "gzip", None).await.unwrap();
    assert_eq!(compressed.headers()["content-encoding"], "gzip");
    assert_eq!(compressed.headers()["etag"], etag);
    let compressed = compressed.into_body().collect().await.unwrap().to_bytes();
    assert!(compressed.starts_with(&[0x1f, 0x8b]));
    assert!(compressed.len() < bytes.len() / 2);
    let unchanged = fetch("/assets/app.js".into(), "gzip", Some(&etag))
        .await
        .unwrap();
    assert_eq!(unchanged.status(), StatusCode::NOT_MODIFIED);
    assert!(
        unchanged
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .is_empty()
    );
    for path in ["/", "/trash", "/api/notes"] {
        let response = fetch(path.into(), "identity", None).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], "no-store");
    }
    for path in [
        "/assets/app.ts",
        "/assets/../Cargo.toml",
        "/settings",
        "/api/export",
    ] {
        assert_eq!(
            fetch(path.into(), "identity", None).await.unwrap().status(),
            StatusCode::NOT_FOUND
        );
    }
    assert_eq!(
        fetch("/favicon.ico".into(), "identity", None)
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn original_attachments_remain_available_without_a_bulk_export_route() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = tungsten::store::Store::open(directory.path()).unwrap();
    let original = vec![7; 1024 * 1024];
    let file = store.upload("original.bin".into(), &original).unwrap();
    drop(store);
    let app = tungsten::api::app(directory.path()).unwrap();
    let (status, _, _) = request(&app, "GET", "/api/export", Value::Null, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/.attachments/{}", file.id))
                .header("host", "localhost")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        original
    );
}

#[tokio::test]
async fn note_lifecycle_persists_markdown_and_trash() {
    let dir = tempfile::tempdir().unwrap();
    let app = tungsten::api::app(dir.path()).unwrap();
    let (status, note, revision) = request(
        &app,
        "POST",
        "/api/notes",
        json!({"name":"買い物.md", "body":"牛乳\n# plain text\n"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(note["name"], "買い物.md");
    let revision = revision.unwrap();
    assert_eq!(
        request(
            &app,
            "GET",
            "/api/notes/%E8%B2%B7%E3%81%84%E7%89%A9.md",
            Value::Null,
            None
        )
        .await
        .1["body"],
        "牛乳\n# plain text\n"
    );
    let (status, found, _) = request(
        &app,
        "GET",
        "/api/notes?q=%E7%89%9B%E4%B9%B3",
        Value::Null,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(found.as_array().unwrap().len(), 1);
    let (_, updated, next) = request(
        &app,
        "PUT",
        "/api/notes/%E8%B2%B7%E3%81%84%E7%89%A9.md",
        json!({"body":"パン\n"}),
        Some(&revision),
    )
    .await;
    assert_eq!(updated["body"], "パン\n");
    let next = next.unwrap();
    assert_ne!(revision, next);
    let (status, _, _) = request(
        &app,
        "PATCH",
        "/api/notes/%E8%B2%B7%E3%81%84%E7%89%A9.md",
        json!({"name":"List.md"}),
        Some(&next),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!dir.path().join("買い物.md").exists());
    let (status, _, _) = request(
        &app,
        "DELETE",
        "/api/notes/List.md",
        Value::Null,
        Some(&next),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(!dir.path().join("List.md").exists());
    drop(app);
    let store = tungsten::store::Store::open(dir.path()).unwrap();
    let trash = store.trash().unwrap();
    assert_eq!(trash.len(), 1);
    assert_eq!(store.saved_note(&trash[0].id, true).unwrap().body, "パン\n");
}

#[tokio::test]
async fn stale_updates_and_colliding_names_do_not_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let app = tungsten::api::app(dir.path()).unwrap();
    let (_, _, revision) = request(
        &app,
        "POST",
        "/api/notes",
        json!({"name":"Note.md", "body":"original"}),
        None,
    )
    .await;
    let revision = revision.unwrap();
    assert_eq!(
        request(
            &app,
            "POST",
            "/api/notes",
            json!({"name":"Note.md", "body":"collision"}),
            None
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let (_, _, next) = request(
        &app,
        "PUT",
        "/api/notes/Note.md",
        json!({"body":"saved"}),
        Some(&revision),
    )
    .await;
    for method in ["PUT", "PATCH", "DELETE"] {
        let body = if method == "PATCH" {
            json!({"name":"Changed.md"})
        } else {
            json!({"body":"stale"})
        };
        assert_eq!(
            request(&app, method, "/api/notes/Note.md", body, Some(&revision))
                .await
                .0,
            StatusCode::PRECONDITION_FAILED
        );
    }
    assert_eq!(
        request(
            &app,
            "PUT",
            "/api/notes/Note.md",
            json!({"body":"missing revision"}),
            None
        )
        .await
        .0,
        StatusCode::PRECONDITION_REQUIRED
    );
    assert_eq!(
        request(&app, "GET", "/api/notes/Note.md", Value::Null, None)
            .await
            .1["body"],
        "saved"
    );
    assert_ne!(next.unwrap(), revision);
}

#[tokio::test]
async fn files_survive_a_service_restart_and_the_directory_has_one_writer() {
    let dir = tempfile::tempdir().unwrap();
    let app = tungsten::api::app(dir.path()).unwrap();
    request(
        &app,
        "POST",
        "/api/notes",
        json!({"name":"Note.md", "body":"persisted"}),
        None,
    )
    .await;
    assert!(tungsten::api::app(dir.path()).is_err());
    drop(app);
    let restarted = tungsten::api::app(dir.path()).unwrap();
    let (status, note, _) =
        request(&restarted, "GET", "/api/notes/Note.md", Value::Null, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(note["body"], "persisted");
}

#[tokio::test]
async fn concurrent_clients_cannot_both_replace_the_same_revision() {
    let dir = tempfile::tempdir().unwrap();
    let app = Arc::new(tungsten::api::app(dir.path()).unwrap());
    let (_, _, revision) = request(
        &app,
        "POST",
        "/api/notes",
        json!({"name":"Note.md", "body":"original"}),
        None,
    )
    .await;
    let revision = revision.unwrap();
    let (a, b) = tokio::join!(
        request(
            &app,
            "PUT",
            "/api/notes/Note.md",
            json!({"body":"a"}),
            Some(&revision)
        ),
        request(
            &app,
            "PUT",
            "/api/notes/Note.md",
            json!({"body":"b"}),
            Some(&revision)
        )
    );
    assert!([a.0, b.0].contains(&StatusCode::OK));
    assert!([a.0, b.0].contains(&StatusCode::PRECONDITION_FAILED));
}

#[tokio::test]
async fn rejects_symlinks_unsafe_names_and_cross_origin_writes() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret"), "private").unwrap();
    let app = tungsten::api::app(dir.path()).unwrap();
    std::os::unix::fs::symlink(outside.path().join("secret"), dir.path().join("Link.md")).unwrap();
    assert_ne!(
        request(&app, "GET", "/api/notes/Link.md", Value::Null, None)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            &app,
            "POST",
            "/api/notes",
            json!({"name":"../outside.md", "body":"escape"}),
            None
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert!(!outside.path().join("outside.md").exists());
    let request = Request::builder()
        .method("POST")
        .uri("/api/notes")
        .header("host", "notes.tail.ts.net")
        .header("origin", "https://evil.test")
        .header("content-type", "application/json")
        .body(Body::from("{\"body\":\"cross origin\"}"))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(request).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    let request = Request::builder()
        .method("POST")
        .uri("/api/notes")
        .header("content-type", "text/plain")
        .body(Body::from("{} "))
        .unwrap();
    assert_eq!(
        app.oneshot(request).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        std::fs::read_to_string(outside.path().join("secret")).unwrap(),
        "private"
    );
}

#[tokio::test]
async fn broken_trash_directory_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join(".trash")).unwrap();
    assert!(tungsten::api::app(Path::new(dir.path())).is_err());
}

#[tokio::test]
async fn rejects_rebound_hosts_and_oversized_notes_without_creating_files() {
    let dir = tempfile::tempdir().unwrap();
    let app = tungsten::api::app(dir.path()).unwrap();
    let untrusted = Request::builder()
        .uri("/api/notes")
        .header("host", "attacker.example")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.clone().oneshot(untrusted).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &app,
            "POST",
            "/api/notes",
            json!({"name":"Large.md", "body":"a".repeat(1024 * 1024 + 1)}),
            None
        )
        .await
        .0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert!(!dir.path().join("Large.md").exists());
    assert_eq!(
        request(&app, "GET", "/api/notes", Value::Null, None)
            .await
            .1,
        json!([])
    );
}

#[tokio::test]
async fn a_rename_collision_preserves_both_notes() {
    let dir = tempfile::tempdir().unwrap();
    let app = tungsten::api::app(dir.path()).unwrap();
    let (_, _, revision) = request(
        &app,
        "POST",
        "/api/notes",
        json!({"name":"Source.md", "body":"source"}),
        None,
    )
    .await;
    request(
        &app,
        "POST",
        "/api/notes",
        json!({"name":"Target.md", "body":"target"}),
        None,
    )
    .await;
    let (status, _, _) = request(
        &app,
        "PATCH",
        "/api/notes/Source.md",
        json!({"name":"Target.md"}),
        revision.as_deref(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        request(&app, "GET", "/api/notes/Source.md", Value::Null, None)
            .await
            .1["body"],
        "source"
    );
    assert_eq!(
        request(&app, "GET", "/api/notes/Target.md", Value::Null, None)
            .await
            .1["body"],
        "target"
    );
}
