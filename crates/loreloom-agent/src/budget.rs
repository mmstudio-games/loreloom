use armillae_core::CompletionResponse;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetReason {
    ModelCalls,
    ToolCalls,
    InputTokens,
    OutputTokens,
    TotalTokens,
    ModelOutputBytes,
    Deadline,
    MissingTokenUsage,
    AgentTurns,
    OrchestrationRounds,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceBudget {
    pub max_model_calls: u32,
    pub max_tool_calls: u32,
    pub max_input_tokens: u64,
    pub max_output_tokens: u64,
    pub max_total_tokens: u64,
    pub max_model_output_bytes: usize,
    pub max_elapsed_ms: u64,
    pub require_reported_tokens: bool,
}

impl Default for ResourceBudget {
    fn default() -> Self {
        Self {
            max_model_calls: 8,
            max_tool_calls: 16,
            max_input_tokens: 131_072,
            max_output_tokens: 16_384,
            max_total_tokens: 147_456,
            max_model_output_bytes: 262_144,
            max_elapsed_ms: 180_000,
            require_reported_tokens: false,
        }
    }
}

impl ResourceBudget {
    #[must_use]
    pub fn strictest(values: impl IntoIterator<Item = Self>) -> Self {
        values.into_iter().reduce(Self::min).unwrap_or_default()
    }

    #[must_use]
    pub fn min(self, other: Self) -> Self {
        Self {
            max_model_calls: self.max_model_calls.min(other.max_model_calls),
            max_tool_calls: self.max_tool_calls.min(other.max_tool_calls),
            max_input_tokens: self.max_input_tokens.min(other.max_input_tokens),
            max_output_tokens: self.max_output_tokens.min(other.max_output_tokens),
            max_total_tokens: self.max_total_tokens.min(other.max_total_tokens),
            max_model_output_bytes: self
                .max_model_output_bytes
                .min(other.max_model_output_bytes),
            max_elapsed_ms: self.max_elapsed_ms.min(other.max_elapsed_ms),
            require_reported_tokens: self.require_reported_tokens || other.require_reported_tokens,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceUsage {
    pub model_calls: u32,
    pub tool_calls: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub model_output_bytes: usize,
    pub missing_token_reports: u32,
}

impl ResourceUsage {
    pub fn merge(&mut self, other: &Self, budget: ResourceBudget) -> Result<(), BudgetReason> {
        self.model_calls = self.model_calls.saturating_add(other.model_calls);
        self.tool_calls = self.tool_calls.saturating_add(other.tool_calls);
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
        self.total_tokens = self.total_tokens.saturating_add(other.total_tokens);
        self.model_output_bytes = self
            .model_output_bytes
            .saturating_add(other.model_output_bytes);
        self.missing_token_reports = self
            .missing_token_reports
            .saturating_add(other.missing_token_reports);
        if self.model_calls > budget.max_model_calls {
            return Err(BudgetReason::ModelCalls);
        }
        if self.tool_calls > budget.max_tool_calls {
            return Err(BudgetReason::ToolCalls);
        }
        if self.input_tokens > budget.max_input_tokens {
            return Err(BudgetReason::InputTokens);
        }
        if self.output_tokens > budget.max_output_tokens {
            return Err(BudgetReason::OutputTokens);
        }
        if self.total_tokens > budget.max_total_tokens {
            return Err(BudgetReason::TotalTokens);
        }
        if self.model_output_bytes > budget.max_model_output_bytes {
            return Err(BudgetReason::ModelOutputBytes);
        }
        if budget.require_reported_tokens && self.missing_token_reports > 0 {
            return Err(BudgetReason::MissingTokenUsage);
        }
        Ok(())
    }

    pub(crate) fn before_model(
        &mut self,
        budget: ResourceBudget,
        elapsed_ms: u64,
    ) -> Result<(), BudgetReason> {
        check_deadline(budget, elapsed_ms)?;
        if self.model_calls >= budget.max_model_calls {
            return Err(BudgetReason::ModelCalls);
        }
        self.model_calls += 1;
        Ok(())
    }

    pub(crate) fn before_tool(
        &mut self,
        budget: ResourceBudget,
        elapsed_ms: u64,
    ) -> Result<(), BudgetReason> {
        check_deadline(budget, elapsed_ms)?;
        if self.tool_calls >= budget.max_tool_calls {
            return Err(BudgetReason::ToolCalls);
        }
        self.tool_calls += 1;
        Ok(())
    }

    pub(crate) fn after_response(
        &mut self,
        budget: ResourceBudget,
        response: &CompletionResponse,
        output_bytes: usize,
        elapsed_ms: u64,
    ) -> Result<(), BudgetReason> {
        check_deadline(budget, elapsed_ms)?;
        self.model_output_bytes = self.model_output_bytes.saturating_add(output_bytes);
        if self.model_output_bytes > budget.max_model_output_bytes {
            return Err(BudgetReason::ModelOutputBytes);
        }
        let Some(usage) = &response.usage else {
            self.missing_token_reports = self.missing_token_reports.saturating_add(1);
            return if budget.require_reported_tokens {
                Err(BudgetReason::MissingTokenUsage)
            } else {
                Ok(())
            };
        };
        self.input_tokens = self
            .input_tokens
            .saturating_add(usage.input_tokens.unwrap_or(0));
        self.output_tokens = self
            .output_tokens
            .saturating_add(usage.output_tokens.unwrap_or(0));
        self.total_tokens = self
            .total_tokens
            .saturating_add(usage.total_tokens.unwrap_or_else(|| {
                usage
                    .input_tokens
                    .unwrap_or(0)
                    .saturating_add(usage.output_tokens.unwrap_or(0))
            }));
        if self.input_tokens > budget.max_input_tokens {
            return Err(BudgetReason::InputTokens);
        }
        if self.output_tokens > budget.max_output_tokens {
            return Err(BudgetReason::OutputTokens);
        }
        if self.total_tokens > budget.max_total_tokens {
            return Err(BudgetReason::TotalTokens);
        }
        Ok(())
    }
}

fn check_deadline(budget: ResourceBudget, elapsed_ms: u64) -> Result<(), BudgetReason> {
    if elapsed_ms > budget.max_elapsed_ms {
        Err(BudgetReason::Deadline)
    } else {
        Ok(())
    }
}
