use crate::{Error, MAX_ATTACHMENT_BYTES, NoteName, Result, Revision, drafts::Draft, types::*};
use reqwest::{Method, RequestBuilder, Response, header};
use serde::de::DeserializeOwned;
use std::{collections::BTreeSet, path::Path, time::Duration};
use tokio::io::AsyncWriteExt;

#[derive(Clone)]
pub struct Client {
    address: ServerAddress,
    http: reqwest::Client,
}

impl Client {
    pub fn new(address: ServerAddress) -> Result<Self> {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
            .user_agent(concat!("Tungsten/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(Error::Network)?;
        Ok(Self { address, http })
    }

    pub fn address(&self) -> &ServerAddress {
        &self.address
    }

    fn request(
        &self,
        method: Method,
        parts: &[&str],
        revision: Option<&Revision>,
    ) -> Result<RequestBuilder> {
        let mut url = reqwest::Url::parse(self.address.as_str()).map_err(|_| Error::Address)?;
        url.path_segments_mut()
            .map_err(|_| Error::Address)?
            .extend(parts.iter().copied());
        let deletion = method == Method::DELETE;
        let mut request = self
            .http
            .request(method, url)
            .header(header::ORIGIN, self.address.as_str())
            .header(header::ACCEPT, "application/json");
        if deletion {
            request = request.header(header::CONTENT_TYPE, "application/json");
        }
        if let Some(revision) = revision {
            request = request.header(header::IF_MATCH, revision.as_str());
        }
        Ok(request)
    }

    async fn send(request: RequestBuilder) -> Result<Response> {
        let response = request.send().await.map_err(Error::Network)?;
        match response.status().as_u16() {
            200..=299 => Ok(response),
            409 | 412 => Err(Error::Conflict),
            404 => Err(Error::Missing),
            413 => Err(Error::TooLarge),
            status => Err(Error::Status(status)),
        }
    }

    async fn json<T: DeserializeOwned>(request: RequestBuilder) -> Result<T> {
        let mut response = Self::send(request).await?;
        const LIMIT: usize = 8 * 1024 * 1024;
        if response
            .content_length()
            .is_some_and(|size| size > LIMIT as u64)
        {
            return Err(Error::TooLarge);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(Error::Network)? {
            if bytes.len().saturating_add(chunk.len()) > LIMIT {
                return Err(Error::TooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| Error::Response)
    }

    pub async fn notes(&self, query: &str, cursor: Option<&str>) -> Result<NotePage> {
        let mut request = self.request(Method::GET, &["api", "notes"], None)?.query(&[
            ("paged", "true"),
            ("limit", "50"),
            ("q", query),
        ]);
        if let Some(cursor) = cursor {
            request = request.query(&[("cursor", cursor)]);
        }
        let page: NotePage = Self::json(request).await?;
        check_page(
            page.notes.iter().map(|note| note.name.as_str()),
            page.next.as_deref(),
            cursor,
        )?;
        if page.total < page.notes.len() {
            return Err(Error::Response);
        }
        Ok(page)
    }

    pub async fn read(&self, name: &NoteName) -> Result<Note> {
        let note: Note =
            Self::json(self.request(Method::GET, &["api", "notes", name.as_str()], None)?).await?;
        note.validate(name)?;
        Ok(note)
    }

    pub async fn save(&self, draft: &Draft) -> Result<Note> {
        draft.validate()?;
        if draft.server != self.address {
            return Err(Error::Address);
        }
        let request = if let Some(revision) = &draft.revision {
            self.request(
                Method::PUT,
                &["api", "notes", draft.name.as_str()],
                Some(revision),
            )?
            .json(&serde_json::json!({ "body": draft.body }))
        } else {
            self.request(Method::POST, &["api", "notes"], None)?
                .json(&serde_json::json!({ "name": draft.name, "body": draft.body }))
        };
        let note: Note = Self::json(request).await?;
        note.validate(&draft.name)?;
        if note.body != draft.body {
            return Err(Error::Response);
        }
        Ok(note)
    }

    pub async fn delete(&self, note: &Note) -> Result<()> {
        Self::send(self.request(
            Method::DELETE,
            &["api", "notes", note.name.as_str()],
            Some(&note.revision),
        )?)
        .await?;
        Ok(())
    }

    pub async fn workspaces(&self, after: Option<&str>) -> Result<WorkspacePage> {
        let mut request = self.request(Method::GET, &["api", "workspaces"], None)?;
        if let Some(after) = after {
            request = request.query(&[("after", after)]);
        }
        let page: WorkspacePage = Self::json(request).await?;
        forward_cursor(page.next.as_deref(), after)?;
        if page.next.as_deref().is_some_and(|next| {
            page.workspaces
                .last()
                .is_none_or(|workspace| workspace.id.as_str() != next)
        }) {
            return Err(Error::Response);
        }
        check_page(
            page.workspaces
                .iter()
                .map(|workspace| workspace.id.as_str()),
            page.next.as_deref(),
            after,
        )?;
        Ok(page)
    }

    pub async fn workspace_notes(
        &self,
        workspace: &WorkspaceId,
        after: Option<&str>,
    ) -> Result<WorkspaceNotes> {
        let mut request = self.request(
            Method::GET,
            &["api", "workspaces", workspace.as_str(), "notes"],
            None,
        )?;
        if let Some(after) = after {
            request = request.query(&[("after", after)]);
        }
        let page: WorkspaceNotes = Self::json(request).await?;
        forward_cursor(page.next.as_deref(), after)?;
        if &page.workspace.id != workspace {
            return Err(Error::Response);
        }
        check_page(
            page.notes.iter().map(|note| note.name.as_str()),
            page.next.as_deref(),
            after,
        )?;
        Ok(page)
    }

    pub async fn create_workspace(&self, title: &str) -> Result<Workspace> {
        Self::json(
            self.request(Method::POST, &["api", "workspaces"], None)?
                .json(&serde_json::json!({ "title": title })),
        )
        .await
    }

    pub async fn rename_workspace(&self, workspace: &Workspace, title: &str) -> Result<Workspace> {
        Self::json(
            self.request(
                Method::PUT,
                &["api", "workspaces", workspace.id.as_str()],
                Some(&workspace.revision),
            )?
            .json(&serde_json::json!({ "title": title })),
        )
        .await
    }

    pub async fn delete_workspace(&self, workspace: &Workspace) -> Result<()> {
        Self::send(self.request(
            Method::DELETE,
            &["api", "workspaces", workspace.id.as_str()],
            Some(&workspace.revision),
        )?)
        .await?;
        Ok(())
    }

    pub async fn membership(
        &self,
        workspace: &WorkspaceId,
        note: &NoteName,
        included: bool,
    ) -> Result<Workspace> {
        let request = self.request(
            if included {
                Method::PUT
            } else {
                Method::DELETE
            },
            &[
                "api",
                "workspaces",
                workspace.as_str(),
                "notes",
                note.as_str(),
            ],
            None,
        )?;
        Self::json(if included {
            request.json(&serde_json::json!({}))
        } else {
            request
        })
        .await
    }

    pub async fn connections(&self, name: &NoteName) -> Result<Connections> {
        let values: Vec<Connections> = Self::json(
            self.request(Method::GET, &["api", "connections"], None)?
                .query(&[("name", name.as_str()), ("limit", "120")]),
        )
        .await?;
        if values.len() > 120 {
            return Err(Error::Response);
        }
        let result = values
            .into_iter()
            .find(|value| &value.name == name)
            .ok_or(Error::Response)?;
        if result
            .attachments
            .iter()
            .any(|id| !tungsten::rules::valid_attachment(id.as_bytes()))
        {
            return Err(Error::Response);
        }
        Ok(result)
    }

    pub async fn backlinks(&self, name: &NoteName) -> Result<Vec<Summary>> {
        let notes: Vec<Summary> = Self::json(
            self.request(Method::GET, &["api", "backlinks"], None)?
                .query(&[("name", name.as_str())]),
        )
        .await?;
        if notes.len() > 120 {
            return Err(Error::Response);
        }
        Ok(notes)
    }

    pub async fn history(&self, name: &NoteName) -> Result<Vec<SavedNote>> {
        let records: Vec<SavedNote> = Self::json(
            self.request(Method::GET, &["api", "history"], None)?
                .query(&[("name", name.as_str())]),
        )
        .await?;
        if records.len() > 50
            || records.iter().any(|record| {
                &record.name != name || !tungsten::rules::valid_id(record.id.as_bytes())
            })
        {
            return Err(Error::Response);
        }
        Ok(records)
    }

    pub async fn saved(&self, record: &SavedNote, trash: bool) -> Result<Note> {
        record_id(record)?;
        let note: Note = Self::json(self.request(
            Method::GET,
            &["api", if trash { "trash" } else { "history" }, &record.id],
            None,
        )?)
        .await?;
        note.validate(&record.name)?;
        Ok(note)
    }

    pub async fn trash(&self, cursor: Option<&str>) -> Result<TrashPage> {
        let mut request = self
            .request(Method::GET, &["api", "trash"], None)?
            .query(&[("page", "true")]);
        if let Some(cursor) = cursor {
            request = request.query(&[("cursor", cursor)]);
        }
        let page: TrashPage = Self::json(request).await?;
        check_page(
            page.entries.iter().map(|record| record.id.as_str()),
            page.next.as_deref(),
            cursor,
        )?;
        for record in &page.entries {
            record_id(record)?;
        }
        Ok(page)
    }

    pub async fn restore_trash(&self, record: &SavedNote, name: &NoteName) -> Result<Note> {
        record_id(record)?;
        let note: Note = Self::json(
            self.request(Method::POST, &["api", "trash", &record.id, "restore"], None)?
                .json(&serde_json::json!({ "name": name })),
        )
        .await?;
        note.validate(name)?;
        Ok(note)
    }

    pub async fn empty_trash(&self) -> Result<()> {
        Self::send(self.request(Method::DELETE, &["api", "trash"], None)?).await?;
        Ok(())
    }

    pub async fn attachment(&self, id: &str) -> Result<Attachment> {
        if !tungsten::rules::valid_attachment(id.as_bytes()) {
            return Err(Error::Identity);
        }
        let value: Attachment =
            Self::json(self.request(Method::GET, &["api", "attachments", id], None)?).await?;
        value.validate()?;
        if value.id != id {
            return Err(Error::Response);
        }
        Ok(value)
    }

    pub async fn upload(&self, path: &Path, name: &str) -> Result<Attachment> {
        let file = tokio::fs::File::open(path).await.map_err(Error::Storage)?;
        let size = file.metadata().await.map_err(Error::Storage)?.len();
        if size > MAX_ATTACHMENT_BYTES {
            return Err(Error::TooLarge);
        }
        let encoded: String = name
            .as_bytes()
            .iter()
            .map(|byte| {
                if byte.is_ascii_alphanumeric() {
                    (*byte as char).to_string()
                } else {
                    format!("%{byte:02X}")
                }
            })
            .collect();
        if encoded.len() > 720 {
            return Err(Error::Identity);
        }
        let request = self
            .request(Method::POST, &["api", "attachments"], None)?
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .header(header::CONTENT_LENGTH, size)
            .header("X-Attachment-Name", encoded)
            .body(reqwest::Body::wrap_stream(
                tokio_util::io::ReaderStream::new(file),
            ));
        let attachment: Attachment = Self::json(request).await?;
        attachment.validate()?;
        Ok(attachment)
    }

    pub async fn download(
        &self,
        attachment: &Attachment,
        directory: &Path,
    ) -> Result<tempfile::TempPath> {
        attachment.validate()?;
        let mut response =
            Self::send(self.request(Method::GET, &[".attachments", &attachment.id], None)?).await?;
        if response
            .content_length()
            .is_some_and(|size| size > MAX_ATTACHMENT_BYTES)
        {
            return Err(Error::TooLarge);
        }
        std::fs::create_dir_all(directory).map_err(Error::Storage)?;
        let extension = Path::new(&attachment.id)
            .extension()
            .and_then(|value| value.to_str())
            .ok_or(Error::Identity)?;
        let path = tempfile::Builder::new()
            .suffix(&format!(".{extension}"))
            .tempfile_in(directory)
            .map_err(Error::Storage)?
            .into_temp_path();
        let mut file = tokio::fs::File::create(&path)
            .await
            .map_err(Error::Storage)?;
        let mut size = 0u64;
        while let Some(chunk) = response.chunk().await.map_err(Error::Network)? {
            size = size.saturating_add(chunk.len() as u64);
            if size > MAX_ATTACHMENT_BYTES {
                return Err(Error::TooLarge);
            }
            file.write_all(&chunk).await.map_err(Error::Storage)?;
        }
        if size != attachment.bytes {
            return Err(Error::Response);
        }
        file.sync_all().await.map_err(Error::Storage)?;
        Ok(path)
    }
}

fn forward_cursor(next: Option<&str>, previous: Option<&str>) -> Result<()> {
    if let Some(next) = next {
        let next = WorkspaceId::try_from(next.to_owned())?;
        let next = next.as_str().parse::<i64>().map_err(|_| Error::Response)?;
        let previous = previous
            .unwrap_or("0")
            .parse::<i64>()
            .map_err(|_| Error::Response)?;
        if next <= previous {
            return Err(Error::Response);
        }
    }
    Ok(())
}

fn record_id(record: &SavedNote) -> Result<()> {
    if tungsten::rules::valid_id(record.id.as_bytes()) {
        Ok(())
    } else {
        Err(Error::Identity)
    }
}

fn check_page<'a>(
    ids: impl Iterator<Item = &'a str>,
    cursor: Option<&str>,
    previous: Option<&str>,
) -> Result<()> {
    let ids: Vec<_> = ids.collect();
    if ids.len() > 50
        || ids.iter().copied().collect::<BTreeSet<_>>().len() != ids.len()
        || cursor
            .is_some_and(|value| Some(value) == previous || value.is_empty() || value.len() > 4096)
    {
        return Err(Error::Response);
    }
    Ok(())
}
