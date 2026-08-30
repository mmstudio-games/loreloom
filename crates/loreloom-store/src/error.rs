use loreloom_core::{DomainError, PersistenceError, RecordError, Revision, RevisionError};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("save directory already exists")]
    SaveAlreadyExists,
    #[error("save directory does not exist")]
    SaveNotFound,
    #[error("save database is not initialized")]
    SaveNotInitialized,
    #[error("save database contains an unexpected number of heads")]
    InvalidSaveHeadCount,
    #[error("store backend operation failed during {operation}")]
    Backend {
        operation: &'static str,
        #[source]
        source: toasty::Error,
    },
    #[error("store JSON codec failed during {operation}")]
    Json {
        operation: &'static str,
        #[source]
        source: serde_json::Error,
    },
    #[error("store integrity check failed: {item}")]
    Integrity { item: &'static str },
    #[error("revision {revision} exceeds the backend integer range")]
    RevisionOutOfRange { revision: Revision },
    #[error("durable commit request is invalid: {field}")]
    InvalidCommit { field: &'static str },
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error(transparent)]
    Record(#[from] RecordError),
    #[error(transparent)]
    Revision(#[from] RevisionError),
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
}

impl StoreError {
    pub(crate) fn backend(operation: &'static str, source: toasty::Error) -> Self {
        Self::Backend { operation, source }
    }

    pub(crate) fn json(operation: &'static str, source: serde_json::Error) -> Self {
        Self::Json { operation, source }
    }
}
