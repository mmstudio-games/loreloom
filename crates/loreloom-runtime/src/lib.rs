//! Loreloom application lifecycle, durable World gateway, and Agent orchestration.

mod config;
mod error;
mod runtime;
mod world_service;

pub use config::{
    NARRATOR_MATERIALIZE_NPC_CAPABILITY, NpcResourcePolicy, OrchestrationBudget, RuntimeConfig,
};
pub use error::RuntimeError;
pub use runtime::{GameRuntime, PlayerTurnOutcome};
pub use world_service::{RuntimeToolExecutor, WorldService};
