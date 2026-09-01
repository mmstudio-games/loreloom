//! Loreloom agent context, orchestration protocols, budgets, and Armillae loop integration.

mod budget;
mod cancellation;
mod failure;
mod protocol;
mod runner;

pub use budget::{BudgetReason, ResourceBudget, ResourceUsage};
pub use cancellation::CancellationToken;
pub use failure::{
    DiagnosticLabel, ModelFailureCategory, ModelFailureDiagnostic, ModelFailureStage,
    ModelInvocationKind,
};
pub use protocol::{
    AgentDefinition, AgentError, AssignmentText, CreateNpcRequest, NarrationText,
    NarrativeImportance, NarratorDefinition, NarratorNpcDecision, NarratorPlan, NpcAgent,
    NpcAssignment, NpcContext, NpcControllerKind, NpcCreationMode, NpcCreationSource,
    NpcGenerationRequest, NpcLifetime, NpcNarrativeAction, NpcTarget, NpcTurnRequest,
    NpcTurnResult, NpcTurnStatus,
};
pub use runner::{
    AgentRunner, AgentToolContext, ToolCallOutcome, TurnFailureStage, TurnInvocation, TurnOutcome,
    TurnStatus,
};
