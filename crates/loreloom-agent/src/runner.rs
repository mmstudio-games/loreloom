use std::{collections::BTreeSet, sync::Arc, time::Instant};

use armillae_core::{AssistantContent, CompletionRequest, Message, ToolResult, ToolResultContent};
use armillae_llm::LlmBridge;
use armillae_tools::{ToolContext, ToolExecutor};
use futures_util::future::{Either, select};
use loreloom_core::{ActorId, EventId, Revision, SessionId};
use serde_json::json;

use crate::{
    BudgetReason, CancellationToken, ModelFailureDiagnostic, ModelFailureStage,
    ModelInvocationKind, ResourceBudget, ResourceUsage,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnFailureStage {
    Projection,
    Provider,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnStatus {
    Completed,
    Cancelled,
    BudgetExhausted(BudgetReason),
    Failed(TurnFailureStage),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnCompletion {
    FinalResponse,
    AfterSuccessfulTool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCallOutcome {
    pub call_id: String,
    pub name: String,
    pub is_error: bool,
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolCallProgress {
    Started { call_id: String, name: String },
    Finished(ToolCallOutcome),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TurnOutcome {
    pub status: TurnStatus,
    pub final_text: Option<String>,
    pub failure: Option<ModelFailureDiagnostic>,
    pub tool_results: Vec<ToolResult>,
    pub tool_calls: Vec<ToolCallOutcome>,
    pub committed_events: Vec<EventId>,
    pub usage: ResourceUsage,
}

#[derive(Debug, Clone)]
pub struct AgentToolContext {
    pub actor_id: ActorId,
    pub revision: Revision,
    pub session_id: SessionId,
    pub capabilities: BTreeSet<String>,
}

pub struct TurnInvocation<'a> {
    pub model_invocation: ModelInvocationKind,
    pub bridge: &'a dyn LlmBridge,
    pub request: CompletionRequest,
    pub tool_context: AgentToolContext,
    pub budget: ResourceBudget,
    pub max_context_tokens: u64,
    pub cancellation: &'a CancellationToken,
    pub completion: TurnCompletion,
}

pub struct AgentRunner {
    executor: Arc<dyn ToolExecutor>,
}

impl std::fmt::Debug for AgentRunner {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AgentRunner")
            .finish_non_exhaustive()
    }
}

impl AgentRunner {
    #[must_use]
    pub fn new(executor: Arc<dyn ToolExecutor>) -> Self {
        Self { executor }
    }

    #[must_use]
    pub fn definitions(&self) -> Vec<armillae_core::ToolDefinition> {
        self.executor.definitions()
    }

    pub async fn run_turn(&self, invocation: TurnInvocation<'_>) -> TurnOutcome {
        self.run_turn_with_progress(invocation, |_| {}).await
    }

    pub async fn run_turn_with_progress<F>(
        &self,
        invocation: TurnInvocation<'_>,
        mut on_tool_progress: F,
    ) -> TurnOutcome
    where
        F: FnMut(ToolCallProgress),
    {
        let TurnInvocation {
            model_invocation,
            bridge,
            mut request,
            mut tool_context,
            budget,
            max_context_tokens,
            cancellation,
            completion,
        } = invocation;
        let offered_tools = request
            .tools
            .iter()
            .map(|definition| definition.name.clone())
            .collect::<BTreeSet<_>>();
        let started = Instant::now();
        let mut usage = ResourceUsage::default();
        let mut tool_results = Vec::new();
        let mut tool_calls = Vec::new();
        let mut committed_events = Vec::new();

        loop {
            if cancellation.is_cancelled() {
                return outcome(
                    TurnStatus::Cancelled,
                    None,
                    tool_results,
                    tool_calls,
                    committed_events,
                    usage,
                );
            }
            let Some(request_tokens) = estimate_request_tokens(&request) else {
                return failure_outcome(
                    TurnStatus::Failed(TurnFailureStage::Projection),
                    ModelFailureDiagnostic::request_encoding(model_invocation),
                    None,
                    tool_results,
                    tool_calls,
                    committed_events,
                    usage,
                );
            };
            if request_tokens > max_context_tokens {
                return outcome(
                    TurnStatus::BudgetExhausted(BudgetReason::InputTokens),
                    None,
                    tool_results,
                    tool_calls,
                    committed_events,
                    usage,
                );
            }
            let elapsed = elapsed_ms(started);
            if let Err(reason) = usage.before_model(budget, elapsed) {
                return outcome(
                    TurnStatus::BudgetExhausted(reason),
                    None,
                    tool_results,
                    tool_calls,
                    committed_events,
                    usage,
                );
            }
            let remaining_output = budget.max_output_tokens.saturating_sub(usage.output_tokens);
            if remaining_output == 0 {
                return outcome(
                    TurnStatus::BudgetExhausted(BudgetReason::OutputTokens),
                    None,
                    tool_results,
                    tool_calls,
                    committed_events,
                    usage,
                );
            }
            request.generation.max_output_tokens = Some(
                request
                    .generation
                    .max_output_tokens
                    .unwrap_or(remaining_output)
                    .min(remaining_output),
            );
            if let Err(error) = bridge.project(&request) {
                return failure_outcome(
                    TurnStatus::Failed(TurnFailureStage::Projection),
                    ModelFailureDiagnostic::from_bridge_error(
                        model_invocation,
                        ModelFailureStage::Projection,
                        &error,
                    ),
                    None,
                    tool_results,
                    tool_calls,
                    committed_events,
                    usage,
                );
            }
            let model_call = bridge.complete(request.clone());
            let cancellation_wait = Box::pin(cancellation.cancelled());
            let response = match select(model_call, cancellation_wait).await {
                Either::Left((Ok(response), _)) => response,
                Either::Left((Err(error), _)) => {
                    return failure_outcome(
                        TurnStatus::Failed(TurnFailureStage::Provider),
                        ModelFailureDiagnostic::from_bridge_error(
                            model_invocation,
                            ModelFailureStage::Invocation,
                            &error,
                        ),
                        None,
                        tool_results,
                        tool_calls,
                        committed_events,
                        usage,
                    );
                }
                Either::Right(((), pending)) => {
                    drop(pending);
                    return outcome(
                        TurnStatus::Cancelled,
                        None,
                        tool_results,
                        tool_calls,
                        committed_events,
                        usage,
                    );
                }
            };
            let response_text = response_text(&response);
            if let Err(reason) =
                usage.after_response(budget, &response, response_text.len(), elapsed_ms(started))
            {
                return outcome(
                    TurnStatus::BudgetExhausted(reason),
                    None,
                    tool_results,
                    tool_calls,
                    committed_events,
                    usage,
                );
            }
            let calls = response.tool_calls().cloned().collect::<Vec<_>>();
            request.messages.push(response.as_assistant_message());
            if calls.is_empty() {
                return outcome(
                    TurnStatus::Completed,
                    Some(response_text),
                    tool_results,
                    tool_calls,
                    committed_events,
                    usage,
                );
            }

            for call in calls {
                if cancellation.is_cancelled() {
                    return outcome(
                        TurnStatus::Cancelled,
                        None,
                        tool_results,
                        tool_calls,
                        committed_events,
                        usage,
                    );
                }
                if let Err(reason) = usage.before_tool(budget, elapsed_ms(started)) {
                    return outcome(
                        TurnStatus::BudgetExhausted(reason),
                        None,
                        tool_results,
                        tool_calls,
                        committed_events,
                        usage,
                    );
                }
                on_tool_progress(ToolCallProgress::Started {
                    call_id: call.id.as_str().to_owned(),
                    name: call.name.clone(),
                });
                let context = ToolContext::new().with_extension(tool_context.clone());
                let result = if offered_tools.contains(call.name.as_str()) {
                    match self.executor.execute(context, call.clone()).await {
                        Ok(result) => result,
                        Err(_) => ToolResult {
                            call_id: call.id.clone(),
                            content: vec![ToolResultContent::Json {
                                value: json!({ "code": "tool_execution_error" }),
                            }],
                            is_error: true,
                        },
                    }
                } else {
                    ToolResult {
                        call_id: call.id.clone(),
                        content: vec![ToolResultContent::Json {
                            value: json!({ "code": "tool_not_offered" }),
                        }],
                        is_error: true,
                    }
                };
                let successful = !result.is_error;
                if successful {
                    collect_events(&result, &mut committed_events);
                    advance_tool_revision(&result, &mut tool_context);
                }
                let error_code = result.is_error.then(|| tool_error_code(&result)).flatten();
                let call_outcome = ToolCallOutcome {
                    call_id: call.id.as_str().to_owned(),
                    name: call.name,
                    is_error: result.is_error,
                    error_code,
                };
                on_tool_progress(ToolCallProgress::Finished(call_outcome.clone()));
                tool_calls.push(call_outcome);
                request.messages.push(Message::tool_result(result.clone()));
                tool_results.push(result);
                if completion == TurnCompletion::AfterSuccessfulTool && successful {
                    return outcome(
                        TurnStatus::Completed,
                        None,
                        tool_results,
                        tool_calls,
                        committed_events,
                        usage,
                    );
                }
            }
        }
    }
}

fn advance_tool_revision(result: &ToolResult, context: &mut AgentToolContext) {
    for content in &result.content {
        if let ToolResultContent::Json { value } = content
            && let Some(revision) = value.get("revision")
            && let Ok(revision) = serde_json::from_value::<Revision>(revision.clone())
            && revision > context.revision
        {
            context.revision = revision;
        }
    }
}

fn tool_error_code(result: &ToolResult) -> Option<String> {
    result.content.iter().find_map(|content| match content {
        ToolResultContent::Json { value } => value
            .get("code")
            .and_then(serde_json::Value::as_str)
            .filter(|code| {
                !code.is_empty()
                    && code.len() <= 64
                    && code.bytes().all(|byte| {
                        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'
                    })
            })
            .map(str::to_owned),
        _ => None,
    })
}

fn response_text(response: &armillae_core::CompletionResponse) -> String {
    response
        .content
        .iter()
        .filter_map(|content| match content {
            AssistantContent::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

fn collect_events(result: &ToolResult, events: &mut Vec<EventId>) {
    for content in &result.content {
        if let ToolResultContent::Json { value } = content {
            if let Some(committed) = value.get("event_ids").and_then(serde_json::Value::as_array) {
                for event in committed {
                    if let Some(event) = event.as_str().and_then(|event| event.parse().ok()) {
                        push_event(events, event);
                    }
                }
            } else if let Some(event) = value
                .get("event_id")
                .and_then(serde_json::Value::as_str)
                .and_then(|event| event.parse().ok())
            {
                push_event(events, event);
            }
        }
    }
}

fn push_event(events: &mut Vec<EventId>, event: EventId) {
    if !events.contains(&event) {
        events.push(event);
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn estimate_request_tokens(request: &CompletionRequest) -> Option<u64> {
    let encoded = serde_json::to_string(request).ok()?;
    let (ascii, non_ascii) = encoded.chars().fold((0_u64, 0_u64), |counts, character| {
        if character.is_ascii() {
            (counts.0.saturating_add(1), counts.1)
        } else {
            (counts.0, counts.1.saturating_add(1))
        }
    });
    Some(ascii.div_ceil(3).saturating_add(non_ascii))
}

fn outcome(
    status: TurnStatus,
    final_text: Option<String>,
    tool_results: Vec<ToolResult>,
    tool_calls: Vec<ToolCallOutcome>,
    committed_events: Vec<EventId>,
    usage: ResourceUsage,
) -> TurnOutcome {
    TurnOutcome {
        status,
        final_text,
        failure: None,
        tool_results,
        tool_calls,
        committed_events,
        usage,
    }
}

fn failure_outcome(
    status: TurnStatus,
    failure: ModelFailureDiagnostic,
    final_text: Option<String>,
    tool_results: Vec<ToolResult>,
    tool_calls: Vec<ToolCallOutcome>,
    committed_events: Vec<EventId>,
    usage: ResourceUsage,
) -> TurnOutcome {
    TurnOutcome {
        status,
        final_text,
        failure: Some(failure),
        tool_results,
        tool_calls,
        committed_events,
        usage,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn error_result(code: &str) -> ToolResult {
        ToolResult {
            call_id: armillae_core::ToolCallId::new("safe-code-test").expect("test tool call ID"),
            content: vec![ToolResultContent::Json {
                value: json!({ "code": code }),
            }],
            is_error: true,
        }
    }

    #[test]
    fn tool_error_codes_are_limited_to_safe_stable_identifiers() {
        assert_eq!(
            tool_error_code(&error_result("invalid_input")).as_deref(),
            Some("invalid_input")
        );
        assert_eq!(tool_error_code(&error_result("InvalidInput")), None);
        assert_eq!(tool_error_code(&error_result("invalid-input")), None);
        assert_eq!(tool_error_code(&error_result("invalid input")), None);
        assert_eq!(tool_error_code(&error_result("")), None);
        assert_eq!(tool_error_code(&error_result(&"x".repeat(65))), None);
    }
}
