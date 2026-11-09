use axum::{
    body::Body,
    http::{Request, StatusCode},
    response::Response,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use tungsten::{
    api::ServerOrigin,
    sealed::{RuntimeManifest, VaultManifest},
};

fn app(path: &std::path::Path) -> Result<axum::Router, tungsten::Error> {
    tungsten::api::sealed_app(path, ServerOrigin::development("http://localhost")?)
}

#[test]
fn refuses_incomplete_backups_and_recovers_only_private_manifest_staging_files() {
    for name in [
        ".sealed.sqlite",
        ".sealed.sqlite-wal",
        ".sealed.sqlite-journal",
    ] {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join(name), b"incomplete backup").unwrap();
        assert!(app(directory.path()).is_err());
        assert!(!directory.path().join(".vault.json").exists());
    }
    let directory = tempfile::tempdir().unwrap();
    let staging = directory
        .path()
        .join(format!(".tungsten-manifest-{}.tmp", "a".repeat(32)));
    std::fs::write(&staging, b"interrupted atomic write").unwrap();
    let opened = app(directory.path()).unwrap();
    assert!(!staging.exists());
    drop(opened);
    let unrelated = directory
        .path()
        .join(format!(".tungsten-{}.tmp", "b".repeat(32)));
    std::fs::write(&unrelated, b"unrelated content").unwrap();
    assert!(app(directory.path()).is_err());
    assert!(unrelated.exists());
    std::fs::remove_file(unrelated).unwrap();
    std::os::unix::fs::symlink(".tungsten.lock", &staging).unwrap();
    assert!(app(directory.path()).is_err());
    assert!(staging.symlink_metadata().is_ok());
}

#[tokio::test]
async fn resumes_pending_initialization_and_reopens_ready_storage_without_rebuilding_indexes() {
    let directory = tempfile::tempdir().unwrap();
    let seed = [63; 32];
    let initial = manifest(&seed);
    std::fs::write(
        directory.path().join(".vault.json"),
        serde_json::to_vec(&initial).unwrap(),
    )
    .unwrap();
    let app = app(directory.path()).unwrap();
    let response = request(
        &app,
        "POST",
        "/api/vault",
        json!({"manifest":initial,"seed":URL_SAFE_NO_PAD.encode(seed)}),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        request(
            &app,
            "POST",
            "/api/vault",
            json!({"manifest":initial,"seed":URL_SAFE_NO_PAD.encode(seed)}),
            None
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    let cookie = response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let created = request(
        &app,
        "POST",
        "/api/notes",
        json!({"name":"recover.md","body":"# Retained body"}),
        Some(&cookie),
    )
    .await;
    let note: Value =
        serde_json::from_slice(&created.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let deletion = Request::delete("/api/notes/recover.md")
        .header("host", "localhost")
        .header("content-type", "application/json")
        .header("cookie", &cookie)
        .header("if-match", note["revision"].as_str().unwrap())
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.clone().oneshot(deletion).await.unwrap().status(),
        StatusCode::NO_CONTENT
    );
    drop(app);
    let database = rusqlite::Connection::open(directory.path().join(".sealed.sqlite")).unwrap();
    let key = initial.database_key(&seed).unwrap();
    let hex: String = key.iter().map(|byte| format!("{byte:02x}")).collect();
    database
        .pragma_update(None, "key", format!("x'{hex}'"))
        .unwrap();
    database.execute_batch("CREATE TRIGGER reject_visibility_rebuild BEFORE INSERT ON trash_visible BEGIN SELECT RAISE(ABORT,'rebuild forbidden'); END;").unwrap();
    drop(database);
    let reopened = self::app(directory.path()).unwrap();
    assert_eq!(
        request(
            &reopened,
            "POST",
            "/api/vault/unlock",
            json!({"seed":URL_SAFE_NO_PAD.encode(seed)}),
            None
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
}

fn manifest(seed: &[u8; 32]) -> RuntimeManifest {
    let wrapped = |length| {
        let mut bytes = vec![84, 86, 76, 84, 1];
        bytes.resize(length, 0);
        URL_SAFE_NO_PAD.encode(bytes)
    };
    RuntimeManifest::create(
        VaultManifest {
            format: 1,
            id: "a".repeat(32),
            key: "b".repeat(32),
            root: wrapped(81),
            check: wrapped(67),
        },
        seed,
    )
    .unwrap()
}

#[tokio::test]
async fn failed_background_migration_stays_locked_and_requires_the_original_source_on_restart() {
    let source = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let mut original = tungsten::store::Store::open(source.path()).unwrap();
    original
        .create(
            Some("original.md".into()),
            "# Preserve source\n\nPrivate body",
        )
        .unwrap();
    let seed = [67; 32];
    let start = || {
        tungsten::api::migration_app(
            target.path(),
            source.path(),
            ServerOrigin::development("http://localhost").unwrap(),
        )
        .unwrap()
    };
    let server = start();
    let opened = request(
        &server,
        "POST",
        "/api/vault",
        json!({"manifest":manifest(&seed),"seed":URL_SAFE_NO_PAD.encode(seed)}),
        None,
    )
    .await;
    assert_eq!(opened.status(), StatusCode::ACCEPTED);
    let durable: Value =
        serde_json::from_slice(&std::fs::read(target.path().join(".vault.json")).unwrap()).unwrap();
    assert_eq!(durable["phase"], "migrating");
    let cookie = opened.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let response = request(&server, "GET", "/api/vault", Value::Null, Some(&cookie)).await;
        let status: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        if status["failed"] == true {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(
        request(
            &server,
            "GET",
            "/api/notes/original.md",
            Value::Null,
            Some(&cookie)
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    drop(server);
    assert!(app(target.path()).is_err());
    drop(original);
    let server = start();
    let opened = request(
        &server,
        "POST",
        "/api/vault/unlock",
        json!({"seed":URL_SAFE_NO_PAD.encode(seed)}),
        None,
    )
    .await;
    assert_eq!(opened.status(), StatusCode::ACCEPTED);
    let cookie = opened.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    loop {
        let response = request(&server, "GET", "/api/vault", Value::Null, Some(&cookie)).await;
        let status: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_ne!(status["failed"], true);
        if status["unlocked"] == true {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(
        request(
            &server,
            "GET",
            "/api/notes/original.md",
            Value::Null,
            Some(&cookie)
        )
        .await
        .status(),
        StatusCode::OK
    );
    drop(server);
    assert!(app(target.path()).is_ok());
    assert!(source.path().join("original.md").is_file());
}

#[tokio::test]
async fn pins_the_deployment_origin_and_cookie_policy_before_accepting_unlock_requests() {
    for origin in [
        "http://notes.example",
        "https://notes.example/path",
        "https://user@notes.example",
        "https://notes.example?query",
        "https://127.0.0.1",
        "https://localhost",
    ] {
        assert!(ServerOrigin::https(origin).is_err(), "{origin}");
    }
    for origin in [
        "http://0.0.0.0:8080",
        "http://notes.example",
        "https://localhost",
    ] {
        assert!(ServerOrigin::development(origin).is_err(), "{origin}");
    }
    let directory = tempfile::tempdir().unwrap();
    let app = tungsten::api::sealed_app(
        directory.path(),
        ServerOrigin::https("https://notes.example").unwrap(),
    )
    .unwrap();
    let seed = [52; 32];
    for (host, origin) in [
        ("localhost", "https://notes.example"),
        ("notes.example:444", "https://notes.example"),
        ("notes.example", "http://notes.example"),
        ("notes.example", "https://notes.example:444"),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::post("/api/vault")
                    .header("host", host)
                    .header("origin", origin)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({"manifest":manifest(&seed),"seed":URL_SAFE_NO_PAD.encode(seed)})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{host} {origin}");
    }
    let response = app
        .oneshot(
            Request::post("/api/vault")
                .header("host", "notes.example")
                .header("origin", "https://notes.example")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"manifest":manifest(&seed),"seed":URL_SAFE_NO_PAD.encode(seed)})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let cookie = response.headers()["set-cookie"].to_str().unwrap();
    assert!(cookie.starts_with("__Host-tungsten-"));
    assert!(cookie.ends_with("; Secure"));
    assert!(!cookie.contains("Domain="));
}

async fn request(
    app: &axum::Router,
    method: &str,
    path: &str,
    input: Value,
    cookie: Option<&str>,
) -> Response {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "localhost")
        .header("content-type", "application/json");
    if let Some(cookie) = cookie {
        request = request.header("cookie", cookie);
    }
    app.clone()
        .oneshot(request.body(Body::from(input.to_string())).unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn unlocks_only_enrolled_devices_and_relocks_every_plaintext_path_after_restart() {
    let directory = tempfile::tempdir().unwrap();
    let seed = [83; 32];
    let manifest = manifest(&seed);
    let app = app(directory.path()).unwrap();
    assert!(self::app(directory.path()).is_err());
    let response = request(&app, "GET", "/api/vault", Value::Null, None).await;
    let state: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(
        state,
        json!({"manifest":null,"unlocked":false,"opening":false,"failed":false})
    );
    let response = request(
        &app,
        "POST",
        "/api/vault",
        json!({"manifest":manifest,"seed":URL_SAFE_NO_PAD.encode(seed)}),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let stored: Value =
        serde_json::from_slice(&std::fs::read(directory.path().join(".vault.json")).unwrap())
            .unwrap();
    assert_eq!(stored["phase"], "ready");
    let cookie = response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .to_owned();
    assert!(cookie.contains("HttpOnly"));
    assert!(cookie.contains("SameSite=Strict"));
    let cookie = cookie.split(';').next().unwrap().to_owned();
    assert_eq!(
        request(
            &app,
            "POST",
            "/api/notes",
            json!({"name":"secret.md","body":"# Private source\n\nStored only as ciphertext"}),
            Some(&cookie)
        )
        .await
        .status(),
        StatusCode::CREATED
    );
    for path in [
        "/api/notes",
        "/api/notes/secret.md",
        "/api/history?name=secret.md",
        "/api/trash",
        "/api/graph",
        "/.attachments/0123456789abcdef0123456789abcdef.txt",
    ] {
        assert!(
            !request(&app, "GET", path, Value::Null, None)
                .await
                .status()
                .is_success()
        );
        assert!(
            !request(&app, "HEAD", path, Value::Null, None)
                .await
                .status()
                .is_success()
        );
    }
    assert_eq!(
        request(
            &app,
            "GET",
            "/api/notes/secret.md",
            Value::Null,
            Some(&cookie)
        )
        .await
        .status(),
        StatusCode::OK
    );
    drop(app);
    assert!(tungsten::api::app(directory.path()).is_err());
    let restarted = self::app(directory.path()).unwrap();
    assert_eq!(
        request(
            &restarted,
            "GET",
            "/api/notes/secret.md",
            Value::Null,
            Some(&cookie)
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &restarted,
            "POST",
            "/api/vault/unlock",
            json!({"seed":URL_SAFE_NO_PAD.encode([84;32])}),
            None
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    let response = request(
        &restarted,
        "POST",
        "/api/vault/unlock",
        json!({"seed":URL_SAFE_NO_PAD.encode(seed)}),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let renewed = response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    assert_ne!(cookie, renewed);
    assert_eq!(
        request(
            &restarted,
            "GET",
            "/api/notes/secret.md",
            Value::Null,
            Some(&renewed)
        )
        .await
        .status(),
        StatusCode::OK
    );
    for entry in std::fs::read_dir(directory.path()).unwrap() {
        let bytes = std::fs::read(entry.unwrap().path()).unwrap();
        for secret in [
            b"Private source".as_slice(),
            b"Stored only as ciphertext".as_slice(),
            &seed,
        ] {
            assert!(!bytes.windows(secret.len()).any(|part| part == secret));
        }
    }
}

#[tokio::test]
async fn completed_initialization_refuses_missing_database_files_without_changing_the_manifest() {
    let directory = tempfile::tempdir().unwrap();
    let seed = [61; 32];
    let app = app(directory.path()).unwrap();
    assert_eq!(
        request(
            &app,
            "POST",
            "/api/vault",
            json!({"manifest":manifest(&seed),"seed":URL_SAFE_NO_PAD.encode(seed)}),
            None
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    drop(app);
    let metadata = std::fs::read(directory.path().join(".vault.json")).unwrap();
    std::fs::remove_file(directory.path().join(".sealed.sqlite")).unwrap();
    assert!(self::app(directory.path()).is_err());
    assert_eq!(
        std::fs::read(directory.path().join(".vault.json")).unwrap(),
        metadata
    );
    assert!(!directory.path().join(".sealed.sqlite").exists());
}
#[tokio::test]
async fn original_file_transport_rejects_unenrolled_bodies_and_streams_bounded_ranges() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let directory = tempfile::tempdir().unwrap();
    let seed = [91; 32];
    let app = app(directory.path()).unwrap();
    let setup = request(
        &app,
        "POST",
        "/api/vault",
        json!({"manifest":manifest(&seed),"seed":URL_SAFE_NO_PAD.encode(seed)}),
        None,
    )
    .await;
    assert_eq!(setup.status(), StatusCode::NO_CONTENT);
    let cookie = setup.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let polled = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&polled);
    let body = Body::from_stream(futures_util::stream::poll_fn(move |_| {
        counter.fetch_add(1, Ordering::Relaxed);
        std::task::Poll::Ready(Some(Ok::<_, std::io::Error>(
            axum::body::Bytes::from_static(b"rejected secret"),
        )))
    }));
    let denied = app
        .clone()
        .oneshot(
            Request::post("/api/attachments")
                .header("host", "localhost")
                .header("content-type", "application/octet-stream")
                .header("x-attachment-name", "private.bin")
                .body(body)
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(polled.load(Ordering::Relaxed), 0);
    let bytes: Vec<u8> = (0..2 * 1024 * 1024)
        .map(|index| (index % 251) as u8)
        .collect();
    let response = app
        .clone()
        .oneshot(
            Request::post("/api/attachments")
                .header("host", "localhost")
                .header("content-type", "application/octet-stream")
                .header("x-attachment-name", "private.bin")
                .header("cookie", &cookie)
                .body(Body::from(bytes.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let file: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let path = file["url"].as_str().unwrap();
    let head = request(&app, "HEAD", path, Value::Null, Some(&cookie)).await;
    assert_eq!(head.headers()["content-length"], bytes.len().to_string());
    assert!(
        head.into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .is_empty()
    );
    let partial = app
        .clone()
        .oneshot(
            Request::get(path)
                .header("host", "localhost")
                .header("cookie", &cookie)
                .header("range", "bytes=1048544-1048607")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(partial.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        partial.into_body().collect().await.unwrap().to_bytes(),
        &bytes[1048544..1048608]
    );
    let mut body = request(&app, "GET", path, Value::Null, Some(&cookie))
        .await
        .into_body();
    let mut received = Vec::new();
    while let Some(frame) = body.frame().await {
        if let Ok(data) = frame.unwrap().into_data() {
            assert!(data.len() <= 64 * 1024);
            received.extend_from_slice(&data);
        }
    }
    assert_eq!(received, bytes);
}
