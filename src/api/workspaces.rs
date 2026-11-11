use super::{SharedStore, expected, not_modified, storage};
use crate::{
    Error,
    domain::{NoteName, Revision},
    store::WorkspaceId,
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, put},
};
use serde::{Deserialize, Serialize};

pub(super) fn routes() -> Router<SharedStore> {
    Router::new()
        .route("/api/workspaces", get(list).post(create))
        .route("/api/workspaces/{id}", get(read).put(rename).delete(delete))
        .route("/api/workspaces/{id}/notes", get(notes))
        .route("/api/workspaces/{id}/notes/{name}", put(add).delete(remove))
        .layer(DefaultBodyLimit::max(4096))
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Page {
    after: Option<String>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Named {
    title: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

fn response<T: Serialize>(
    status: StatusCode,
    value: &T,
    revision: &Revision,
    fresh: bool,
) -> Result<Response, Error> {
    let mut response = if fresh {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        (status, Json(value)).into_response()
    };
    response.headers_mut().insert(
        header::ETAG,
        HeaderValue::from_str(revision.as_str()).map_err(|_| Error::Worker)?,
    );
    Ok(response)
}

async fn list(
    State(state): State<SharedStore>,
    Query(page): Query<Page>,
    headers: HeaderMap,
) -> Result<Response, Error> {
    let value = storage(state, move |store| {
        store.workspaces(page.after.as_deref(), page.limit.unwrap_or(50))
    })
    .await?;
    response(
        StatusCode::OK,
        &value,
        &value.revision,
        not_modified(&headers, &value.revision),
    )
}

async fn create(
    State(state): State<SharedStore>,
    Json(input): Json<Named>,
) -> Result<Response, Error> {
    let value = storage(state, move |store| store.create_workspace(&input.title)).await?;
    response(StatusCode::CREATED, &value, &value.revision, false)
}

async fn read(
    State(state): State<SharedStore>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, Error> {
    let id = WorkspaceId::parse(&id)?;
    let value = storage(state, move |store| store.workspace(id)).await?;
    response(
        StatusCode::OK,
        &value,
        &value.revision,
        not_modified(&headers, &value.revision),
    )
}

async fn rename(
    State(state): State<SharedStore>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(input): Json<Named>,
) -> Result<Response, Error> {
    let id = WorkspaceId::parse(&id)?;
    let revision = expected(&headers)?;
    let value = storage(state, move |store| {
        store.rename_workspace(id, &input.title, &revision)
    })
    .await?;
    response(StatusCode::OK, &value, &value.revision, false)
}

async fn delete(
    State(state): State<SharedStore>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, Error> {
    let id = WorkspaceId::parse(&id)?;
    let revision = expected(&headers)?;
    storage(state, move |store| store.delete_workspace(id, &revision)).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn add(
    State(state): State<SharedStore>,
    Path((id, name)): Path<(String, String)>,
    Json(_input): Json<Empty>,
) -> Result<Response, Error> {
    member(state, id, name, true).await
}

async fn remove(
    State(state): State<SharedStore>,
    Path((id, name)): Path<(String, String)>,
) -> Result<Response, Error> {
    member(state, id, name, false).await
}

async fn member(
    state: SharedStore,
    id: String,
    name: String,
    included: bool,
) -> Result<Response, Error> {
    let id = WorkspaceId::parse(&id)?;
    let name = NoteName::parse(name)?;
    let value = storage(state, move |store| {
        store.workspace_member(id, &name, included)
    })
    .await?;
    response(StatusCode::OK, &value, &value.revision, false)
}

async fn notes(
    State(state): State<SharedStore>,
    Path(id): Path<String>,
    Query(page): Query<Page>,
    headers: HeaderMap,
) -> Result<Response, Error> {
    let id = WorkspaceId::parse(&id)?;
    let after = page.after;
    let limit = page.limit.unwrap_or(50);
    let value = storage(state, move |store| {
        store.workspace_notes(id, after.as_deref(), limit)
    })
    .await?;
    let revision = value.revision.clone();
    response(
        StatusCode::OK,
        &value,
        &revision,
        not_modified(&headers, &revision),
    )
}
