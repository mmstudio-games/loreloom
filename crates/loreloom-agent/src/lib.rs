//! Loreloom agent context, orchestration protocols, budgets, and Armillae loop integration.

mod budget;
mod cancellation;
mod protocol;
mod runner;

pub use budget::{BudgetReason, ResourceBudget, ResourceUsage};
pub use cancellation::CancellationToken;
pub use protocol::{
    AgentDefinition, AgentError, AssignmentText, NarrationText, NarrativeImportance,
    NarratorNpcDecision, NarratorPlan, NpcAgent, NpcAssignment, NpcContext, NpcControllerKind,
    NpcGenerationRequest, NpcLifetime, NpcNarrativeAction, NpcTarget, NpcTurnRequest,
    NpcTurnResult, NpcTurnStatus,
};
pub use runner::{
    AgentRunner, AgentToolContext, ToolCallOutcome, TurnFailureStage, TurnInvocation, TurnOutcome,
    TurnStatus,
};
