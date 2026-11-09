use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    response::Response,
};
use base64::{Engine, prelude::BASE64_STANDARD};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

async fn request(
    app: &Router,
    method: &str,
    path: &str,
    input: Value,
    headers: &[(&str, &str)],
) -> Response {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "localhost")
        .header("content-type", "application/json");
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    app.clone()
        .oneshot(request.body(Body::from(input.to_string())).unwrap())
        .await
        .unwrap()
}

async fn json(response: Response) -> Value {
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

#[tokio::test]
async fn recordings_and_documents_keep_original_bytes_and_support_seeking() {
    let directory = tempfile::tempdir().unwrap();
    let app = tungsten::api::app(directory.path()).unwrap();
    for (name, bytes, mime, kind) in [
        (
            "recording.wav",
            b"RIFF0000WAVEfmt data".as_slice(),
            "audio/wav",
            "audio",
        ),
        (
            "report.pdf",
            b"%PDF-1.7\noriginal".as_slice(),
            "application/pdf",
            "pdf",
        ),
        (
            "fake.pdf",
            b"<script>unsafe</script>".as_slice(),
            "application/octet-stream",
            "file",
        ),
    ] {
        let file = json(
            request(
                &app,
                "POST",
                "/api/attachments",
                json!({"name":name,"data":BASE64_STANDARD.encode(bytes)}),
                &[],
            )
            .await,
        )
        .await;
        assert_eq!(file["kind"], kind);
        let url = file["url"].as_str().unwrap();
        let response = request(&app, "GET", url, Value::Null, &[]).await;
        assert_eq!(response.headers()["content-type"], mime);
        assert_eq!(response.headers()["accept-ranges"], "bytes");
        let etag = response.headers()["etag"].to_str().unwrap().to_owned();
        let head = request(&app, "HEAD", url, Value::Null, &[("range", "bytes=0-3")]).await;
        assert_eq!(head.status(), StatusCode::OK);
        assert_eq!(head.headers()["content-length"], bytes.len().to_string());
        assert_eq!(
            response.into_body().collect().await.unwrap().to_bytes(),
            bytes
        );
        for (range, content, selected) in [
            (
                "bytes=0-3",
                format!("bytes 0-3/{}", bytes.len()),
                &bytes[..4],
            ),
            (
                "bytes=-4",
                format!(
                    "bytes {}-{}/{}",
                    bytes.len() - 4,
                    bytes.len() - 1,
                    bytes.len()
                ),
                &bytes[bytes.len() - 4..],
            ),
        ] {
            let response = request(
                &app,
                "GET",
                url,
                Value::Null,
                &[("range", range), ("if-range", &etag)],
            )
            .await;
            assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
            assert_eq!(response.headers()["content-range"], content);
            assert_eq!(
                response.into_body().collect().await.unwrap().to_bytes(),
                selected
            );
        }
        let response = request(&app, "GET", url, Value::Null, &[("range", "bytes=9999-")]).await;
        assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(
            response.headers()["content-range"],
            format!("bytes */{}", bytes.len())
        );
        let response = request(
            &app,
            "GET",
            url,
            Value::Null,
            &[("range", "bytes=0-3"), ("if-range", "\"stale\"")],
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let response = request(&app, "GET", &format!("{url}?download=1"), Value::Null, &[]).await;
        assert!(
            response.headers()["content-disposition"]
                .to_str()
                .unwrap()
                .starts_with("attachment")
        );
    }
}

#[tokio::test]
async fn retired_file_management_preserves_reading_existing_attachment_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let app = tungsten::api::app(directory.path()).unwrap();
    let file = json(
        request(
            &app,
            "POST",
            "/api/attachments",
            json!({"name":"original.txt","data":"aGVsbG8="}),
            &[],
        )
        .await,
    )
    .await;
    let id = file["id"].as_str().unwrap();
    let path = format!("/api/attachments/{id}");
    for method in ["PATCH"] {
        assert_eq!(
            request(&app, method, &path, json!({}), &[]).await.status(),
            StatusCode::METHOD_NOT_ALLOWED
        );
    }
    assert_eq!(
        request(&app, "GET", "/api/attachments", Value::Null, &[])
            .await
            .status(),
        StatusCode::METHOD_NOT_ALLOWED
    );
    std::fs::write(
        directory.path().join(format!(".attachments/{id}.json")),
        br#"{"name":"Renamed.txt","folder":"Projects/Research","description":"A transcript"}"#,
    )
    .unwrap();

    drop(app);
    let app = tungsten::api::app(directory.path()).unwrap();
    let restored = json(request(&app, "GET", &path, Value::Null, &[]).await).await;
    assert_eq!(restored["name"], "Renamed.txt");
    std::fs::write(
        directory.path().join(format!(".attachments/{id}.json")),
        b"\"Legacy.txt\"",
    )
    .unwrap();
    let legacy = json(request(&app, "GET", &path, Value::Null, &[]).await).await;
    assert_eq!(legacy["name"], "Legacy.txt");
    assert_eq!(legacy["folder"], "");
    assert_eq!(
        request(
            &app,
            "DELETE",
            &path,
            Value::Null,
            &[("if-match", "\"stale\"")]
        )
        .await
        .status(),
        StatusCode::PRECONDITION_FAILED
    );
    assert_eq!(
        request(
            &app,
            "DELETE",
            &path,
            Value::Null,
            &[("if-match", legacy["revision"].as_str().unwrap())]
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(&app, "GET", file["url"].as_str().unwrap(), Value::Null, &[])
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
}
#[tokio::test]
async fn uploads_original_bytes_without_base64_and_validates_the_file_transport_boundary() {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let directory = tempfile::tempdir().unwrap();
    let app = tungsten::api::app(directory.path()).unwrap();
    let bytes = b"original file bytes";
    let response = app
        .clone()
        .oneshot(
            Request::post("/api/attachments")
                .header("host", "localhost")
                .header("content-type", "application/octet-stream")
                .header("x-attachment-name", "original%20file.txt")
                .body(Body::from(bytes.as_slice()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let file: serde_json::Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(file["name"], "original file.txt");
    let response = app
        .clone()
        .oneshot(
            Request::get(file["url"].as_str().unwrap())
                .header("host", "localhost")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        bytes.as_slice()
    );
    for (path, origin) in [
        ("/api/notes", "http://localhost"),
        ("/api/attachments", "https://foreign.test"),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::post(path)
                    .header("host", "localhost")
                    .header("origin", origin)
                    .header("content-type", "application/octet-stream")
                    .header("x-attachment-name", "refused.txt")
                    .body(Body::from(bytes.as_slice()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}
