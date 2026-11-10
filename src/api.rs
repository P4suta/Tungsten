use crate::{
    Error,
    domain::NoteName,
    store::{MAX_NOTE_BYTES, Note, Store},
};
use axum::{
    Extension, Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Path as UrlPath, Query, State},
    http::{HeaderMap, HeaderValue, Method, Request, StatusCode, Uri, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};
use std::{path::Path, sync::Arc};
use tokio::sync::Mutex;

type SharedStore = Arc<Mutex<Store>>;
mod features;
mod maintenance;
mod origin;
mod vendor;
mod workspaces;

pub use origin::ServerOrigin;

pub fn app_with_origin(data_dir: &Path, origin: ServerOrigin) -> Result<Router, Error> {
    Ok(app(data_dir)?.layer(Extension(origin)))
}

pub fn app(data_dir: &Path) -> Result<Router, Error> {
    with_store(
        Store::open(data_dir)?,
        uuid::Uuid::new_v4().simple().to_string(),
    )
}

fn with_store(store: Store, nonce: String) -> Result<Router, Error> {
    let state = Arc::new(Mutex::new(store));
    let maintenance = Arc::new(maintenance::Maintenance::start(Arc::downgrade(&state))?);
    Ok(Router::new()
        .route("/", get(index))
        .route("/trash", get(index))
        .route("/favicon.ico", get(|| async { StatusCode::NO_CONTENT }))
        .route("/assets/{*path}", get(browser_asset))
        .route("/healthz", get(|| async { StatusCode::NO_CONTENT }))
        .route("/vendor/pdf/{*path}", get(vendor::pdf))
        .merge(features::routes())
        .merge(workspaces::routes())
        .route("/api/notes", get(list).post(create))
        .route(
            "/api/notes/{name}",
            get(read).put(update).patch(rename).delete(delete),
        )
        .layer(DefaultBodyLimit::max(MAX_NOTE_BYTES * 6 + 4096))
        .layer(tower_http::compression::CompressionLayer::new())
        .layer(middleware::from_fn(boundary))
        .layer(Extension(nonce))
        .layer(Extension(maintenance))
        .with_state(state))
}

async fn index(Extension(nonce): Extension<String>) -> Html<String> {
    Html(include_str!("../ui/index.html").replace("__STYLE_NONCE__", &nonce))
}

static CHUNKS: include_dir::Dir<'_> =
    include_dir::include_dir!("$CARGO_MANIFEST_DIR/ui/dist/chunks");

async fn browser_asset(UrlPath(path): UrlPath<String>, headers: HeaderMap) -> Response {
    let (bytes, mime) = match path.as_str() {
        "style.css" => (
            include_bytes!("../ui/style.css").as_slice(),
            "text/css; charset=utf-8",
        ),
        "pdf.mjs" => (
            include_bytes!("../ui/node_modules/pdfjs-dist/build/pdf.mjs").as_slice(),
            "text/javascript; charset=utf-8",
        ),
        "app.js" => (
            include_bytes!("../ui/dist/app.js").as_slice(),
            "text/javascript; charset=utf-8",
        ),
        path if path.starts_with("chunks/") && path.ends_with(".js") => {
            let Some(file) = CHUNKS.get_file(path.strip_prefix("chunks/").unwrap_or(path)) else {
                return StatusCode::NOT_FOUND.into_response();
            };
            (file.contents(), "text/javascript; charset=utf-8")
        }
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    let revision = crate::domain::Revision::of(bytes);
    let fresh = not_modified(&headers, &revision);
    let mut response = if fresh {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        ([(header::CONTENT_TYPE, mime)], bytes).into_response()
    };
    response.headers_mut().insert(
        header::ETAG,
        HeaderValue::from_str(&format!("W/{}", revision.as_str())).expect("hexadecimal revision"),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(if path.starts_with("chunks/") {
            "public, max-age=31536000, immutable"
        } else {
            "public, max-age=0, must-revalidate"
        }),
    );
    response
}

async fn boundary(
    Extension(nonce): Extension<String>,
    origin: Option<Extension<ServerOrigin>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    if !origin.as_ref().map_or_else(
        || valid_host(request.headers()),
        |Extension(origin)| origin.permits(request.headers()),
    ) {
        return Error::Origin.into_response();
    }
    if request.uri().path().starts_with("/api/")
        && !matches!(
            *request.method(),
            Method::GET | Method::HEAD | Method::OPTIONS
        )
        && !valid_origin(
            request.headers(),
            request.uri().path() == "/api/attachments" && *request.method() == Method::POST,
        )
    {
        return Error::Origin.into_response();
    }
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers
        .entry(header::CACHE_CONTROL)
        .or_insert(HeaderValue::from_static("no-store"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    if let Ok(policy) = HeaderValue::from_str(&format!(
        "default-src 'none'; script-src 'self'; worker-src 'self'; style-src 'self' 'nonce-{nonce}'; connect-src 'self'; img-src 'self' blob:; media-src 'self'; font-src 'self' blob:; base-uri 'none'; frame-ancestors 'none'; form-action 'none'"
    )) {
        headers.insert(header::CONTENT_SECURITY_POLICY, policy);
    }
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    response
}

fn valid_host(headers: &HeaderMap) -> bool {
    let Some(authority) = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<axum::http::uri::Authority>().ok())
    else {
        return false;
    };
    if authority.as_str().contains('@') {
        return false;
    }
    let host = authority.host().to_ascii_lowercase();
    host == "localhost"
        || host.ends_with(".ts.net")
        || host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<std::net::IpAddr>()
            .is_ok()
}

fn valid_origin(headers: &HeaderMap, allow_binary: bool) -> bool {
    let json = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value.split(';').next().is_some_and(|mime| {
                mime.trim().eq_ignore_ascii_case("application/json")
                    || (allow_binary
                        && mime.trim().eq_ignore_ascii_case("application/octet-stream"))
            })
        });
    if !json
        || headers
            .get("sec-fetch-site")
            .is_some_and(|site| site != "same-origin" && site != "none")
    {
        return false;
    }
    let Some(origin) = headers.get(header::ORIGIN) else {
        return true;
    };
    let Ok(origin) = origin.to_str().unwrap_or_default().parse::<Uri>() else {
        return false;
    };
    matches!(origin.scheme_str(), Some("http" | "https"))
        && origin.authority().is_some_and(|authority| {
            headers
                .get(header::HOST)
                .is_some_and(|host| host.as_bytes() == authority.as_str().as_bytes())
        })
}

async fn storage<T: Send + 'static>(
    state: SharedStore,
    operation: impl FnOnce(&mut Store) -> Result<T, Error> + Send + 'static,
) -> Result<T, Error> {
    let mut store = state.lock_owned().await;
    tokio::task::spawn_blocking(move || operation(&mut store))
        .await
        .map_err(|_| Error::Worker)?
}

fn not_modified(headers: &HeaderMap, revision: &crate::domain::Revision) -> bool {
    headers.get_all(header::IF_NONE_MATCH).iter().any(|header| {
        header.to_str().is_ok_and(|values| {
            values.split(',').any(|value| {
                let value = value.trim();
                value == "*" || value.strip_prefix("W/").unwrap_or(value) == revision.as_str()
            })
        })
    })
}

fn note_response(status: StatusCode, note: Note) -> Result<Response, Error> {
    let revision = HeaderValue::from_str(note.revision.as_str()).map_err(|_| Error::Worker)?;
    Ok((status, [(header::ETAG, revision)], Json(note)).into_response())
}

fn expected(headers: &HeaderMap) -> Result<String, Error> {
    headers
        .get(header::IF_MATCH)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .ok_or(Error::Precondition)
}

#[derive(Default, Deserialize)]
struct Search {
    #[serde(default)]
    q: String,
    cursor: Option<String>,
    limit: Option<usize>,
    #[serde(default)]
    paged: bool,
    #[serde(default)]
    titles: bool,
}

async fn list(
    State(state): State<SharedStore>,
    Query(search): Query<Search>,
) -> Result<Response, Error> {
    let page = storage(state, move |store| {
        store.search(
            &search.q,
            search.cursor.as_deref(),
            search.limit.unwrap_or(50),
            search.titles,
        )
    })
    .await?;
    Ok(if search.paged {
        Json(page).into_response()
    } else {
        Json(page.notes).into_response()
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Create {
    name: Option<String>,
    body: String,
}

async fn create(
    State(state): State<SharedStore>,
    Json(input): Json<Create>,
) -> Result<Response, Error> {
    note_response(
        StatusCode::CREATED,
        storage(state, move |store| store.create(input.name, &input.body)).await?,
    )
}

async fn read(
    State(state): State<SharedStore>,
    UrlPath(name): UrlPath<String>,
    headers: HeaderMap,
) -> Result<Response, Error> {
    let name = NoteName::parse(name)?;
    let note = storage(state, move |store| store.read_durable(&name)).await?;
    if not_modified(&headers, &note.revision) {
        let revision = HeaderValue::from_str(note.revision.as_str()).map_err(|_| Error::Worker)?;
        return Ok((StatusCode::NOT_MODIFIED, [(header::ETAG, revision)]).into_response());
    }
    note_response(StatusCode::OK, note)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Update {
    body: String,
}

async fn update(
    State(state): State<SharedStore>,
    UrlPath(name): UrlPath<String>,
    headers: HeaderMap,
    Json(input): Json<Update>,
) -> Result<Response, Error> {
    let name = NoteName::parse(name)?;
    let revision = expected(&headers)?;
    note_response(
        StatusCode::OK,
        storage(state, move |store| {
            store.update(&name, &input.body, &revision)
        })
        .await?,
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rename {
    name: String,
}

async fn rename(
    State(state): State<SharedStore>,
    UrlPath(name): UrlPath<String>,
    headers: HeaderMap,
    Json(input): Json<Rename>,
) -> Result<Response, Error> {
    let name = NoteName::parse(name)?;
    let renamed = NoteName::parse(input.name)?;
    let revision = expected(&headers)?;
    note_response(
        StatusCode::OK,
        storage(state, move |store| store.rename(&name, &renamed, &revision)).await?,
    )
}

async fn delete(
    State(state): State<SharedStore>,
    UrlPath(name): UrlPath<String>,
    headers: HeaderMap,
) -> Result<StatusCode, Error> {
    let name = NoteName::parse(name)?;
    let revision = expected(&headers)?;
    storage(state, move |store| store.delete(&name, &revision)).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
struct Failure {
    error: String,
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = match self {
            Self::InvalidName | Self::InvalidFile | Self::InvalidData(_) => StatusCode::BAD_REQUEST,
            Self::Exists | Self::WorkspaceExists => StatusCode::CONFLICT,
            Self::NotFound | Self::WorkspaceMissing => StatusCode::NOT_FOUND,
            Self::Conflict | Self::WorkspaceChanged => StatusCode::PRECONDITION_FAILED,
            Self::Precondition => StatusCode::PRECONDITION_REQUIRED,
            Self::TooLarge | Self::Limit(_) => StatusCode::PAYLOAD_TOO_LARGE,
            Self::Origin => StatusCode::FORBIDDEN,
            Self::SearchLimit | Self::UploadBusy => StatusCode::SERVICE_UNAVAILABLE,
            Self::Locked | Self::Io(_) | Self::Catalog(_) | Self::Worker => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        };
        let error = if matches!(self, Self::SearchLimit | Self::UploadBusy) {
            self.to_string()
        } else if status.is_server_error() {
            eprintln!("operation=storage outcome=failed error={self}");
            "Storage operation failed. Keep your draft and retry.".to_owned()
        } else {
            self.to_string()
        };
        (status, Json(Failure { error })).into_response()
    }
}
