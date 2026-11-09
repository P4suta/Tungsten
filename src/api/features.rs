use super::{SharedStore, expected, note_response, storage};
use crate::{Error, domain::NoteName, markdown, store::MAX_NOTE_BYTES};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, Method, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::{Engine, prelude::BASE64_STANDARD};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::{Deserialize, Serialize};

pub(super) fn routes() -> Router<SharedStore> {
    Router::new()
        .route("/api/preview", post(preview))
        .route("/api/backlinks", get(backlinks))
        .route("/api/connections", get(connections))
        .route("/api/graph", get(graph))
        .route("/api/pins", post(pin))
        .route("/api/history", get(history))
        .route("/api/history/{id}", get(history_note))
        .route("/api/history/{id}/restore", post(restore_history))
        .route("/api/trash", get(trash).delete(empty_trash))
        .route("/api/trash/{id}", get(trash_note).delete(purge_trash))
        .route("/api/trash/{id}/restore", post(restore_trash))
        .route(
            "/api/attachments",
            post(upload)
                .layer::<_, std::convert::Infallible>(DefaultBodyLimit::max(90 * 1024 * 1024))
                .layer(axum::Extension(std::sync::Arc::new(UploadSlots(
                    tokio::sync::Semaphore::new(1),
                )))),
        )
        .route(
            "/api/attachments/{id}",
            get(attachment_info).delete(forget_attachment),
        )
        .route("/.attachments/{id}", get(attachment))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PreviewInput {
    #[serde(default)]
    name: String,
    body: String,
}

async fn preview(Json(input): Json<PreviewInput>) -> Result<Response, Error> {
    if input.body.len() > MAX_NOTE_BYTES {
        return Err(Error::TooLarge);
    }
    let name = if input.name.is_empty() {
        "Untitled.md".to_owned()
    } else {
        NoteName::parse(input.name)?.as_str().to_owned()
    };
    let result = tokio::task::spawn_blocking(move || markdown::preview(&name, &input.body))
        .await
        .map_err(|_| Error::Worker)?;
    Ok(Json(result).into_response())
}

#[derive(Deserialize)]
struct Named {
    name: String,
}

#[derive(Serialize)]
struct Connection {
    name: NoteName,
    links: Vec<String>,
    attachments: Vec<String>,
}

#[derive(Default, Deserialize)]
struct GraphInput {
    name: Option<String>,
    limit: Option<usize>,
    attachment: Option<String>,
}

async fn graph(
    State(state): State<SharedStore>,
    Query(input): Query<GraphInput>,
) -> Result<Response, Error> {
    let focus = input.name.map(NoteName::parse).transpose()?;
    Ok(Json(
        storage(state, move |store| {
            store.graph(focus.as_ref(), input.limit.unwrap_or(120))
        })
        .await?,
    )
    .into_response())
}

async fn connections(
    State(state): State<SharedStore>,
    Query(input): Query<GraphInput>,
) -> Result<Response, Error> {
    Ok(Json(
        storage(state, move |store| {
            let summaries = if let Some(id) = input.attachment {
                store.attachment_sources(&id)?
            } else if let Some(name) = input.name {
                store
                    .graph(Some(&NoteName::parse(name)?), input.limit.unwrap_or(120))?
                    .notes
            } else {
                store.graph(None, input.limit.unwrap_or(120))?.notes
            };
            summaries
                .into_iter()
                .map(|summary| {
                    let (links, attachments) = store.connection(&summary.name)?;
                    Ok(Connection {
                        name: summary.name,
                        links,
                        attachments,
                    })
                })
                .collect::<Result<Vec<_>, Error>>()
        })
        .await?,
    )
    .into_response())
}

async fn backlinks(
    State(state): State<SharedStore>,
    Query(input): Query<Named>,
) -> Result<Response, Error> {
    let name = NoteName::parse(input.name)?;
    let notes = storage(state, move |store| store.backlinks(&name)).await?;
    Ok(Json(notes).into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PinInput {
    name: String,
    pinned: bool,
}

async fn pin(
    State(state): State<SharedStore>,
    headers: HeaderMap,
    Json(input): Json<PinInput>,
) -> Result<StatusCode, Error> {
    let name = NoteName::parse(input.name)?;
    let revision = expected(&headers)?;
    storage(state, move |store| {
        store.pin(&name, input.pinned, &revision)
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn history(
    State(state): State<SharedStore>,
    Query(input): Query<Named>,
) -> Result<Response, Error> {
    let name = NoteName::parse(input.name)?;
    Ok(Json(storage(state, move |store| store.history(&name)).await?).into_response())
}

async fn history_note(
    State(state): State<SharedStore>,
    Path(id): Path<String>,
) -> Result<Response, Error> {
    note_response(
        StatusCode::OK,
        storage(state, move |store| store.saved_note(&id, false)).await?,
    )
}

async fn trash_note(
    State(state): State<SharedStore>,
    Path(id): Path<String>,
) -> Result<Response, Error> {
    note_response(
        StatusCode::OK,
        storage(state, move |store| store.saved_note(&id, true)).await?,
    )
}

async fn restore_history(
    State(state): State<SharedStore>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(input): Json<Named>,
) -> Result<Response, Error> {
    let name = NoteName::parse(input.name)?;
    let revision = expected(&headers)?;
    note_response(
        StatusCode::OK,
        storage(state, move |store| {
            store.restore_history(&id, &name, &revision)
        })
        .await?,
    )
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct TrashInput {
    page: bool,
    cursor: Option<String>,
}

async fn trash(
    State(state): State<SharedStore>,
    Query(input): Query<TrashInput>,
) -> Result<Response, Error> {
    if input.page {
        Ok(Json(
            storage(state, move |store| {
                store.trash_page(input.cursor.as_deref())
            })
            .await?,
        )
        .into_response())
    } else {
        Ok(Json(storage(state, |store| store.trash()).await?).into_response())
    }
}

async fn empty_trash(State(state): State<SharedStore>) -> Result<StatusCode, Error> {
    storage(state, |store| store.empty_trash()).await?;
    Ok(StatusCode::ACCEPTED)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RestoreInput {
    name: Option<String>,
}

async fn restore_trash(
    State(state): State<SharedStore>,
    Path(id): Path<String>,
    Json(input): Json<RestoreInput>,
) -> Result<Response, Error> {
    note_response(
        StatusCode::CREATED,
        storage(state, move |store| store.restore_trash(&id, input.name)).await?,
    )
}

async fn purge_trash(
    State(state): State<SharedStore>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, Error> {
    let revision = expected(&headers)?;
    storage(state, move |store| store.purge_trash(&id, &revision)).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UploadInput {
    name: String,
    data: String,
}

struct UploadSlots(tokio::sync::Semaphore);

async fn upload(
    State(state): State<SharedStore>,
    axum::Extension(slots): axum::Extension<std::sync::Arc<UploadSlots>>,
    request: axum::extract::Request,
) -> Result<Response, Error> {
    use axum::extract::FromRequest;
    let _permit = slots.0.try_acquire().map_err(|_| Error::UploadBusy)?;
    if request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|mime| {
            mime.split(';').next().is_some_and(|value| {
                value
                    .trim()
                    .eq_ignore_ascii_case("application/octet-stream")
            })
        })
    {
        let encoded = request
            .headers()
            .get("x-attachment-name")
            .and_then(|value| value.to_str().ok())
            .filter(|value| value.len() <= 720)
            .ok_or(Error::InvalidData("The attachment name is invalid."))?;
        let name = percent_encoding::percent_decode_str(encoded)
            .decode_utf8()
            .map_err(|_| Error::InvalidData("The attachment name is invalid."))?
            .into_owned();
        if request
            .headers()
            .get(header::CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .is_some_and(|length| length > 64 * 1024 * 1024)
        {
            return Err(Error::Limit("Attachments must be at most 64 MiB."));
        }
        let bytes = axum::body::to_bytes(request.into_body(), 64 * 1024 * 1024)
            .await
            .map_err(|_| {
                Error::InvalidData("The attachment could not be read within its 64 MiB limit.")
            })?;
        return Ok((
            StatusCode::CREATED,
            Json(storage(state, move |store| store.upload(name, &bytes)).await?),
        )
            .into_response());
    }
    let Json(input) = match Json::<UploadInput>::from_request(request, &()).await {
        Ok(input) => input,
        Err(error) => return Ok(error.into_response()),
    };
    if input.data.len() > 86 * 1024 * 1024 {
        return Err(Error::Limit("Attachments must be at most 64 MiB."));
    }
    let bytes = BASE64_STANDARD
        .decode(input.data)
        .map_err(|_| Error::InvalidData("The attachment is not valid base64."))?;
    Ok((
        StatusCode::CREATED,
        Json(storage(state, move |store| store.upload(input.name, &bytes)).await?),
    )
        .into_response())
}

fn file_response(
    bytes: axum::body::Body,
    name: &str,
    mime: &str,
    inline: bool,
) -> Result<Response, Error> {
    let disposition = if inline { "inline" } else { "attachment" };
    let disposition = format!(
        "{disposition}; filename*=UTF-8''{}",
        utf8_percent_encode(name, NON_ALPHANUMERIC)
    );
    Ok((
        [
            (header::CONTENT_TYPE, mime.to_owned()),
            (header::CONTENT_DISPOSITION, disposition),
        ],
        bytes,
    )
        .into_response())
}

async fn attachment(
    State(state): State<SharedStore>,
    Path(id): Path<String>,
    Query(input): Query<AttachmentQuery>,
    headers: HeaderMap,
    method: Method,
) -> Result<Response, Error> {
    let attachment = storage(state.clone(), move |store| store.attachment(&id)).await?;
    use crate::media::{ByteRange, MediaKind, byte_range};
    let length = attachment.bytes;
    let etag = &attachment.data_revision;
    let range = if method == Method::GET
        && headers
            .get(header::IF_RANGE)
            .is_none_or(|value| value.to_str().ok() == Some(etag.as_str()))
    {
        byte_range(
            headers
                .get(header::RANGE)
                .and_then(|value| value.to_str().ok()),
            length,
        )
    } else {
        ByteRange::Full
    };
    let (status, content_range, range) = match range {
        ByteRange::Full => (StatusCode::OK, None, 0..length),
        ByteRange::Partial(range) => (
            StatusCode::PARTIAL_CONTENT,
            Some(format!("bytes {}-{}/{length}", range.start, range.end - 1)),
            range,
        ),
        ByteRange::Unsatisfiable => (
            StatusCode::RANGE_NOT_SATISFIABLE,
            Some(format!("bytes */{length}")),
            0..0,
        ),
    };
    let content_length = range.len();
    let body = if method == Method::HEAD || range.is_empty() {
        axum::body::Body::empty()
    } else {
        let id = attachment.id.clone();
        let stream =
            futures_util::stream::try_unfold((state, id, range), |(state, id, range)| async move {
                if range.is_empty() {
                    return Ok::<_, Error>(None);
                }
                let end = range.end.min(range.start + 64 * 1024);
                let next = range.start..end;
                let selected = id.clone();
                let bytes = storage(state.clone(), move |store| {
                    store.attachment_range(&selected, next)
                })
                .await?;
                Ok(Some((
                    axum::body::Bytes::from(bytes),
                    (state, id, end..range.end),
                )))
            });
        axum::body::Body::from_stream(stream)
    };
    let mut response = file_response(
        body,
        &attachment.name,
        attachment.mime,
        input.download != Some(1) && attachment.kind != MediaKind::File,
    )?;
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::ETAG,
        etag.as_str().parse().map_err(|_| Error::Worker)?,
    );
    response.headers_mut().insert(
        header::CONTENT_LENGTH,
        content_length
            .to_string()
            .parse()
            .map_err(|_| Error::Worker)?,
    );
    response.headers_mut().insert(
        header::ACCEPT_RANGES,
        "bytes".parse().map_err(|_| Error::Worker)?,
    );
    if let Some(value) = content_range {
        response.headers_mut().insert(
            header::CONTENT_RANGE,
            value.parse().map_err(|_| Error::Worker)?,
        );
    }
    Ok(response)
}

#[derive(Deserialize)]
struct AttachmentQuery {
    download: Option<u8>,
}

async fn attachment_info(
    State(state): State<SharedStore>,
    Path(id): Path<String>,
) -> Result<Response, Error> {
    Ok(Json(storage(state, move |store| store.attachment(&id)).await?).into_response())
}

async fn forget_attachment(
    State(state): State<SharedStore>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, Error> {
    let revision = expected(&headers)?;
    storage(state, move |store| store.delete_attachment(&id, &revision)).await?;
    Ok(StatusCode::NO_CONTENT)
}
