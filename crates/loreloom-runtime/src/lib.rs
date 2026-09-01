//! Loreloom application lifecycle, durable World gateway, and Agent orchestration.

mod config;
mod context;
mod error;
mod runtime;
mod world_service;

pub use config::{
    ContextProjectionPolicy, NARRATOR_CREATE_NPC_CAPABILITY, NARRATOR_CREATE_PLACE_CAPABILITY,
    NARRATOR_CREATE_SCENE_CAPABILITY, NARRATOR_REQUEST_NPC_TURN_CAPABILITY,
    NARRATOR_SUBMIT_NPC_DRAFT_CAPABILITY, NARRATOR_TRANSITION_SCENE_CAPABILITY, NpcResourcePolicy,
    OrchestrationBudget, RuntimeConfig,
};
pub use error::RuntimeError;
pub use runtime::{GameRuntime, PlayerTurnOutcome};
pub use world_service::{RuntimeToolExecutor, WorldService};
