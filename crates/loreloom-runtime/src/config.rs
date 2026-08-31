use std::collections::BTreeSet;

use loreloom_agent::ResourceBudget;

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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RuntimeConfig {
    pub turn_budget: ResourceBudget,
    pub orchestration_budget: OrchestrationBudget,
    pub narrator_capabilities: BTreeSet<String>,
}
