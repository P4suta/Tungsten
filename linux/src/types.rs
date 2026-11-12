use crate::{Error, MAX_ATTACHMENT_BYTES, MAX_NOTE_BYTES, NoteName, Result, Revision};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ServerAddress(String);

impl ServerAddress {
    pub fn parse(value: &str, development: bool) -> Result<Self> {
        let url = reqwest::Url::parse(value.trim()).map_err(|_| Error::Address)?;
        let host = url.host_str().ok_or(Error::Address)?;
        let local = host == "localhost"
            || host
                .trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback());
        if !(url.scheme() == "https" || (development && local && url.scheme() == "http"))
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/"
            || url.port() == Some(0)
        {
            return Err(Error::Address);
        }
        Ok(Self(url.origin().ascii_serialization()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ServerAddress {
    type Error = Error;
    fn try_from(value: String) -> Result<Self> {
        Self::parse(&value, cfg!(debug_assertions))
    }
}

impl From<ServerAddress> for String {
    fn from(value: ServerAddress) -> Self {
        value.0
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct Note {
    pub name: NoteName,
    pub body: String,
    pub revision: Revision,
}

impl Note {
    pub fn validate(&self, name: &NoteName) -> Result<()> {
        if &self.name != name {
            return Err(Error::Response);
        }
        if self.body.len() > MAX_NOTE_BYTES {
            return Err(Error::TooLarge);
        }
        Ok(())
    }
}

pub fn title(body: &str) -> String {
    body.lines()
        .find_map(|line| line.strip_prefix("# "))
        .or_else(|| body.lines().find(|line| !line.trim().is_empty()))
        .unwrap_or("Untitled Note")
        .trim()
        .to_owned()
}

pub fn generated_name() -> NoteName {
    NoteName::parse(format!("{}.md", uuid::Uuid::new_v4().simple()))
        .expect("UUID note names satisfy the shared path rules")
}

#[derive(Clone, Debug, Deserialize)]
pub struct Summary {
    pub name: NoteName,
    pub title: String,
    #[serde(default)]
    pub snippet: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct NotePage {
    pub notes: Vec<Summary>,
    pub next: Option<String>,
    pub total: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct WorkspaceId(String);

impl TryFrom<String> for WorkspaceId {
    type Error = Error;
    fn try_from(value: String) -> Result<Self> {
        let number = value.parse::<i64>().map_err(|_| Error::Identity)?;
        if number <= 0 || number.to_string() != value {
            return Err(Error::Identity);
        }
        Ok(Self(value))
    }
}
impl From<WorkspaceId> for String {
    fn from(value: WorkspaceId) -> Self {
        value.0
    }
}
impl WorkspaceId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub title: String,
    pub revision: Revision,
}

#[derive(Clone, Debug, Deserialize)]
pub struct WorkspacePage {
    pub workspaces: Vec<Workspace>,
    pub next: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct WorkspaceNotes {
    pub workspace: Workspace,
    pub notes: Vec<Summary>,
    pub next: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SavedNote {
    pub id: String,
    pub name: NoteName,
    pub title: String,
    pub captured: u64,
    pub revision: Revision,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TrashPage {
    pub entries: Vec<SavedNote>,
    pub next: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Connections {
    pub name: NoteName,
    pub links: Vec<NoteName>,
    pub attachments: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Attachment {
    pub id: String,
    pub name: String,
    pub bytes: u64,
    pub revision: Revision,
}

impl Attachment {
    pub fn validate(&self) -> Result<()> {
        if !tungsten::rules::valid_attachment(self.id.as_bytes()) {
            return Err(Error::Identity);
        }
        if self.bytes > MAX_ATTACHMENT_BYTES {
            return Err(Error::TooLarge);
        }
        Ok(())
    }
}
