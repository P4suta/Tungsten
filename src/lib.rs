#[cfg(feature = "server")]
pub mod api;
#[cfg(feature = "storage")]
pub mod backup;
pub mod domain;
#[cfg(feature = "storage")]
pub mod export;
pub mod markdown;
pub mod media;
pub mod rules;
#[cfg(feature = "storage")]
pub mod store;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(
        "Use a visible relative Markdown path of at most 240 bytes without hidden, empty, or parent components."
    )]
    InvalidName,
    #[error("A note with that name already exists.")]
    Exists,
    #[error("The note was not found.")]
    NotFound,
    #[error("The saved note changed. Reload it or save your draft as a copy.")]
    Conflict,
    #[error("The workspace was removed or is unavailable.")]
    WorkspaceMissing,
    #[error("The workspace changed. Reload it and retry.")]
    WorkspaceChanged,
    #[error("A workspace with that name already exists.")]
    WorkspaceExists,
    #[error("An If-Match revision is required.")]
    Precondition,
    #[error("The note exceeds the 1 MiB limit.")]
    TooLarge,
    #[error("The note is not a regular UTF-8 file.")]
    InvalidFile,
    #[error("Invalid file data: {0}")]
    InvalidData(&'static str),
    #[error("File limit exceeded: {0}")]
    Limit(&'static str),
    #[error("The request must come from the app origin and use JSON.")]
    Origin,
    #[error("The data directory is already in use.")]
    Locked,
    #[error("Storage operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("The storage worker is unavailable.")]
    Worker,
    #[error("The catalog operation failed: {0}")]
    #[cfg(feature = "storage")]
    Catalog(#[from] rusqlite::Error),
    #[error("The search is too expensive. Use a more specific query.")]
    SearchLimit,
    #[error("Another file is uploading. Try again after it finishes.")]
    UploadBusy,
}
