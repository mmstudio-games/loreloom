use loreloom_core::{
    ContentDefinitionId, DomainError, FixedError, IdentityError, ObjectId, Revision, RevisionError,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum WorldError {
    #[error("expected revision {expected}, observed {observed}")]
    Conflict {
        expected: Revision,
        observed: Revision,
    },
    #[error("object {id} does not exist")]
    ObjectNotFound { id: ObjectId },
    #[error("object {id} has the wrong domain kind")]
    WrongObjectKind { id: ObjectId },
    #[error("definition {id} does not exist or has the wrong kind")]
    DefinitionNotFound { id: ContentDefinitionId },
    #[error("world identity is duplicated")]
    DuplicateIdentity,
    #[error("world state record is missing or duplicated")]
    WorldState,
    #[error("domain rule rejected the command: {rule}")]
    DomainRule { rule: &'static str },
    #[error("world invariant failed: {invariant}")]
    Invariant { invariant: &'static str },
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error(transparent)]
    Content(#[from] loreloom_content::ContentError),
    #[error(transparent)]
    Revision(#[from] RevisionError),
    #[error(transparent)]
    Fixed(#[from] FixedError),
    #[error(transparent)]
    Identity(#[from] IdentityError),
}
