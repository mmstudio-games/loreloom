use std::collections::{BTreeMap, BTreeSet};

use loreloom_agent::ResourceBudget;
use loreloom_content::GenerationPolicy;
use loreloom_core::ContentDefinitionId;

pub const NARRATOR_MATERIALIZE_NPC_CAPABILITY: &str = "narrator.materialize_npc";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrchestrationBudget {
    pub resources: ResourceBudget,
    pub max_started_agent_turns: u32,
    pub max_orchestration_rounds: u32,
}

impl Default for OrchestrationBudget {
    fn default() -> Self {
        Self {
            resources: ResourceBudget {
                max_model_calls: 64,
                max_tool_calls: 128,
                max_input_tokens: 1_048_576,
                max_output_tokens: 131_072,
                max_total_tokens: 1_179_648,
                max_model_output_bytes: 2_097_152,
                max_elapsed_ms: 900_000,
                require_reported_tokens: false,
            },
            max_started_agent_turns: 24,
            max_orchestration_rounds: 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NpcResourcePolicy {
    pub max_generated_per_orchestration: u32,
    pub max_materialized_per_scene: u32,
    pub max_persistent_generated: u32,
}

impl Default for NpcResourcePolicy {
    fn default() -> Self {
        Self {
            max_generated_per_orchestration: 32,
            max_materialized_per_scene: 256,
            max_persistent_generated: 1_024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeConfig {
    pub turn_budget: ResourceBudget,
    pub orchestration_budget: OrchestrationBudget,
    pub narrator_capabilities: BTreeSet<String>,
    pub npc_resources: NpcResourcePolicy,
    pub generation_policies: BTreeMap<ContentDefinitionId, GenerationPolicy>,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            turn_budget: ResourceBudget::default(),
            orchestration_budget: OrchestrationBudget::default(),
            narrator_capabilities: BTreeSet::from([NARRATOR_MATERIALIZE_NPC_CAPABILITY.to_owned()]),
            npc_resources: NpcResourcePolicy::default(),
            generation_policies: BTreeMap::new(),
        }
    }
}
