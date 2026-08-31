use loreloom_agent::{AgentError, BudgetReason};
use loreloom_core::{IdentityError, RevisionError, TextError};
use loreloom_store::StoreError;
use loreloom_world::WorldError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("player input is empty or invalid")]
    InvalidInput,
    #[error("runtime actor or scene is unavailable")]
    Unavailable,
    #[error("save ModLock does not match the configured content packages")]
    ContentLockMismatch,
    #[error("agent model protocol is invalid at {stage}")]
    ModelProtocol { stage: &'static str },
    #[error("agent bridge is unavailable")]
    BridgeUnavailable,
    #[error("agent capability is not authorized")]
    CapabilityDenied,
    #[error("agent orchestration budget exhausted: {0:?}")]
    Budget(BudgetReason),
    #[error("operation was cancelled")]
    Cancelled,
    #[error(transparent)]
    Agent(#[from] AgentError),
    #[error(transparent)]
    Identity(#[from] IdentityError),
    #[error(transparent)]
    Revision(#[from] RevisionError),
    #[error(transparent)]
    Text(#[from] TextError),
    #[error(transparent)]
    World(#[from] WorldError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("runtime JSON codec failed at {stage}")]
    Json {
        stage: &'static str,
        #[source]
        source: serde_json::Error,
    },
}

impl RuntimeError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidInput => "invalid_input",
            Self::Unavailable => "unavailable",
            Self::ContentLockMismatch => "content_lock_mismatch",
            Self::ModelProtocol { .. } | Self::Agent(_) | Self::Json { .. } => "agent_error",
            Self::BridgeUnavailable => "bridge_unavailable",
            Self::CapabilityDenied => "capability_denied",
            Self::Budget(_) => "budget_exhausted",
            Self::Cancelled => "cancelled",
            Self::Identity(_) => "identity_error",
            Self::Revision(_) => "revision_error",
            Self::Text(_) => "text_error",
            Self::World(WorldError::Conflict { .. }) => "revision_conflict",
            Self::World(WorldError::DomainRule { .. }) => "domain_rule",
            Self::World(_) => "world_error",
            Self::Store(_) => "store_error",
        }
    }

    pub(crate) fn json(stage: &'static str, source: serde_json::Error) -> Self {
        Self::Json { stage, source }
    }
}
