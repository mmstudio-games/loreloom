use std::collections::BTreeSet;

use loreloom_agent::ResourceBudget;
use loreloom_content::GenerationPolicy;
use serde::{Deserialize, Serialize};

pub const NARRATOR_CREATE_NPC_CAPABILITY: &str = "narrator.create_npc";
pub const NARRATOR_CREATE_PLACE_CAPABILITY: &str = "narrator.create_place";
pub const NARRATOR_CREATE_SCENE_CAPABILITY: &str = "narrator.create_scene";
pub const NARRATOR_REQUEST_NPC_TURN_CAPABILITY: &str = "narrator.request_npc_turn";
pub const NARRATOR_SUBMIT_NPC_DRAFT_CAPABILITY: &str = "narrator.submit_npc_draft";
pub const NARRATOR_TRANSITION_SCENE_CAPABILITY: &str = "narrator.transition_scene";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ContextProjectionPolicy {
    pub transcript_items: usize,
    pub transcript_bytes: usize,
    pub known_facts: usize,
    pub goals: usize,
    pub visible_actors: usize,
    pub inventory_items: usize,
    pub skills: usize,
    pub max_context_tokens: u64,
}

impl Default for ContextProjectionPolicy {
    fn default() -> Self {
        Self {
            transcript_items: 64,
            transcript_bytes: 64 * 1_024,
            known_facts: 256,
            goals: 64,
            visible_actors: 128,
            inventory_items: 128,
            skills: 64,
            max_context_tokens: 32_768,
        }
    }
}

impl ContextProjectionPolicy {
    const MAXIMUM: Self = Self {
        transcript_items: 256,
        transcript_bytes: 256 * 1_024,
        known_facts: 1_024,
        goals: 256,
        visible_actors: 512,
        inventory_items: 512,
        skills: 256,
        max_context_tokens: 131_072,
    };

    pub fn validate(self) -> Result<(), &'static str> {
        let maximum = Self::MAXIMUM;
        if self.transcript_items > maximum.transcript_items {
            return Err("context_projection.transcript_items");
        }
        if self.transcript_bytes > maximum.transcript_bytes {
            return Err("context_projection.transcript_bytes");
        }
        if self.known_facts > maximum.known_facts {
            return Err("context_projection.known_facts");
        }
        if self.goals > maximum.goals {
            return Err("context_projection.goals");
        }
        if self.visible_actors > maximum.visible_actors {
            return Err("context_projection.visible_actors");
        }
        if self.inventory_items > maximum.inventory_items {
            return Err("context_projection.inventory_items");
        }
        if self.skills > maximum.skills {
            return Err("context_projection.skills");
        }
        if self.max_context_tokens == 0 || self.max_context_tokens > maximum.max_context_tokens {
            return Err("context_projection.max_context_tokens");
        }
        Ok(())
    }
}

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
                max_model_calls: 128,
                max_tool_calls: 512,
                max_input_tokens: 4_194_304,
                max_output_tokens: 524_288,
                max_total_tokens: 4_718_592,
                max_model_output_bytes: 8_388_608,
                max_elapsed_ms: 1_800_000,
                require_reported_tokens: false,
            },
            max_started_agent_turns: 48,
            max_orchestration_rounds: 8,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_budgets_allow_repeated_tool_continuations_and_orchestration() {
        let turn = ResourceBudget::default();
        assert_eq!(turn.max_model_calls, 16);
        assert_eq!(turn.max_tool_calls, 64);
        assert_eq!(turn.max_input_tokens, 524_288);
        assert_eq!(turn.max_output_tokens, 32_768);
        assert_eq!(turn.max_total_tokens, 557_056);
        assert_eq!(turn.max_model_output_bytes, 1_048_576);
        assert_eq!(turn.max_elapsed_ms, 600_000);

        let orchestration = OrchestrationBudget::default();
        assert_eq!(orchestration.max_started_agent_turns, 48);
        assert_eq!(orchestration.max_orchestration_rounds, 8);
        assert_eq!(orchestration.resources.max_model_calls, 128);
        assert_eq!(orchestration.resources.max_tool_calls, 512);
        assert_eq!(orchestration.resources.max_input_tokens, 4_194_304);
        assert_eq!(orchestration.resources.max_output_tokens, 524_288);
        assert_eq!(orchestration.resources.max_total_tokens, 4_718_592);
        assert_eq!(orchestration.resources.max_model_output_bytes, 8_388_608);
        assert_eq!(orchestration.resources.max_elapsed_ms, 1_800_000);
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
    pub generation_policy: Option<GenerationPolicy>,
    pub context_projection: ContextProjectionPolicy,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            turn_budget: ResourceBudget::default(),
            orchestration_budget: OrchestrationBudget::default(),
            narrator_capabilities: BTreeSet::from([
                NARRATOR_CREATE_NPC_CAPABILITY.to_owned(),
                NARRATOR_CREATE_PLACE_CAPABILITY.to_owned(),
                NARRATOR_CREATE_SCENE_CAPABILITY.to_owned(),
                NARRATOR_REQUEST_NPC_TURN_CAPABILITY.to_owned(),
                NARRATOR_TRANSITION_SCENE_CAPABILITY.to_owned(),
            ]),
            npc_resources: NpcResourcePolicy::default(),
            generation_policy: None,
            context_projection: ContextProjectionPolicy::default(),
        }
    }
}
