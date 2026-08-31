//! Loreloom ECS state, rules, factories, and simulation integration.

mod components;
mod error;
mod game_world;

pub use components::{ObjectKind, PersistentId};
pub use error::WorldError;
pub use game_world::{GameWorld, RuleLimits, WorldBootstrap, WorldConfig};
