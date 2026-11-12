pub mod client;
pub mod drafts;
pub mod editor;
pub mod types;

pub use tungsten::domain::{NoteName, Revision};

pub const APP_ID: &str = "io.github.p4suta.tungsten";
pub const MAX_NOTE_BYTES: usize = 1024 * 1024;
pub const MAX_ATTACHMENT_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Enter an HTTPS server address without a path, password, query, or fragment.")]
    Address,
    #[error("The server returned an invalid response.")]
    Response,
    #[error("The item changed on another device. Reload it or save the draft as a new note.")]
    Conflict,
    #[error("The name is already in use. Choose another name.")]
    Exists,
    #[error("The item is no longer available. Save the draft as a new note.")]
    Missing,
    #[error("The server could not complete the request (HTTP {0}).")]
    Status(u16),
    #[error("Unable to reach the server. Local drafts have been kept.")]
    Network(#[source] reqwest::Error),
    #[error("Unable to save the draft on this device. Keep the editor open and retry.")]
    Storage(#[source] std::io::Error),
    #[error("Some drafts could not be read. Their original files have been kept.")]
    Recovery,
    #[error("Notes must be at most 1 MiB and attachments at most 64 MiB.")]
    TooLarge,
    #[error("The saved identifier is invalid.")]
    Identity,
    #[error("Wait for the current operation to finish.")]
    Busy,
    #[error("The editor generation is exhausted. Keep the editor open.")]
    Generation,
}

pub type Result<T> = std::result::Result<T, Error>;
