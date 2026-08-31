use std::{
    future::poll_fn,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    task::Poll,
};

use armillae_core::{
    AssistantContent, CompletionRequest, CompletionResponse, ContentPart, FinishReason, Message,
    ToolCall, ToolCallId, ToolDefinition, ToolResult, ToolResultContent,
};
use armillae_llm::{
    BoxFuture as BridgeFuture, BridgeCapabilities, BridgeError, CompletionStream, LlmBridge,
    MockBridge, MockResponse, ProjectionReport,
};
use armillae_tools::{BoxFuture as ToolFuture, ToolContext, ToolExecutionError, ToolExecutor};
use futures_executor::block_on;
use loreloom_agent::{
    AgentRunner, AgentToolContext, BudgetReason, CancellationToken, NarratorPlan, ResourceBudget,
    TurnInvocation, TurnStatus,
};
use loreloom_core::{ActorId, Revision, SessionId};
use serde_json::json;

fn parse<T: std::str::FromStr>(value: &str) -> T
where
    T::Err: std::fmt::Debug,
{
    value.parse().expect("fixture identifier")
}

fn call(id: &str, name: &str) -> ToolCall {
    ToolCall {
        id: ToolCallId::new(id).expect("tool call id"),
        name: name.to_owned(),
        arguments: json!({}),
    }
}

fn tool_response(calls: Vec<ToolCall>) -> CompletionResponse {
    CompletionResponse {
        id: None,
        model: Some("mock-agent".to_owned()),
        content: calls.into_iter().map(AssistantContent::ToolCall).collect(),
        finish_reason: Some(FinishReason::ToolCall),
        usage: None,
        provider_metadata: serde_json::Value::Null,
    }
}

fn tool_definitions() -> Vec<ToolDefinition> {
    ["commit_first", "inspect_after"]
        .into_iter()
        .map(|name| ToolDefinition {
            name: name.to_owned(),
            description: name.to_owned(),
            input_schema: json!({ "type": "object", "additionalProperties": false }),
        })
        .collect()
}

#[derive(Default)]
struct RecordingExecutor {
    calls: Mutex<Vec<(String, Revision)>>,
}

impl RecordingExecutor {
    fn calls(&self) -> Vec<(String, Revision)> {
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl ToolExecutor for RecordingExecutor {
    fn definitions(&self) -> Vec<ToolDefinition> {
        tool_definitions()
    }

    fn execute<'a>(
        &'a self,
        context: ToolContext,
        call: ToolCall,
    ) -> ToolFuture<'a, Result<ToolResult, ToolExecutionError>> {
        Box::pin(async move {
            let runtime = context.get::<AgentToolContext>().ok_or_else(|| {
                ToolExecutionError::ExecutionFailed {
                    name: call.name.clone(),
                    message: "missing runtime context".to_owned(),
                }
            })?;
            self.calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push((call.name.clone(), runtime.revision));
            let value = match call.name.as_str() {
                "commit_first" => json!({
                    "revision": 2,
                    "event_id": "evt_01890f6a-2b40-7d4e-8f90-123456789abc",
                    "event_ids": [
                        "evt_01890f6a-2b40-7d4e-8f90-123456789abc",
                        "evt_01890f6a-2b41-7d4e-8f90-123456789abc"
                    ]
                }),
                "inspect_after" => json!({ "observed_revision": runtime.revision }),
                _ => {
                    return Err(ToolExecutionError::UnknownTool {
                        name: call.name.clone(),
                    });
                }
            };
            Ok(ToolResult {
                call_id: call.id,
                content: vec![ToolResultContent::Json { value }],
                is_error: false,
            })
        })
    }
}

fn invocation<'a>(
    bridge: &'a dyn LlmBridge,
    cancellation: &'a CancellationToken,
    budget: ResourceBudget,
) -> TurnInvocation<'a> {
    TurnInvocation {
        bridge,
        request: CompletionRequest {
            messages: vec![Message::user("act")],
            tools: tool_definitions(),
            ..CompletionRequest::default()
        },
        tool_context: AgentToolContext {
            actor_id: parse::<ActorId>("obj_01890f6a-2b3c-7d4e-8f90-123456789abc"),
            revision: Revision::new(1),
            session_id: parse::<SessionId>("ses_01890f6a-2b3d-7d4e-8f90-123456789abc"),
            capabilities: Default::default(),
        },
        budget,
        max_context_tokens: u64::MAX,
        cancellation,
    }
}

#[test]
fn product_runner_rejects_a_known_tool_that_was_not_offered_to_the_model() {
    let executor = Arc::new(RecordingExecutor::default());
    let runner = AgentRunner::new(executor.clone());
    let bridge = MockBridge::scripted([
        MockResponse::completion(tool_response(vec![call("hidden-call", "commit_first")])),
        MockResponse::text("done"),
    ]);
    let cancellation = CancellationToken::new();
    let mut invocation = invocation(&bridge, &cancellation, ResourceBudget::default());
    invocation.request.tools.clear();

    let outcome = block_on(runner.run_turn(invocation));

    assert_eq!(outcome.status, TurnStatus::Completed);
    assert!(executor.calls().is_empty());
    assert!(outcome.tool_results[0].is_error);
    assert!(matches!(
        &outcome.tool_results[0].content[..],
        [ToolResultContent::Json { value }] if value["code"] == json!("tool_not_offered")
    ));
}

#[test]
fn product_runner_rejects_an_oversized_projected_context_before_provider_io() {
    let executor = Arc::new(RecordingExecutor::default());
    let runner = AgentRunner::new(executor);
    let bridge = MockBridge::scripted([MockResponse::text("must not be called")]);
    let cancellation = CancellationToken::new();
    let mut invocation = invocation(&bridge, &cancellation, ResourceBudget::default());
    invocation.max_context_tokens = 1;

    let outcome = block_on(runner.run_turn(invocation));

    assert_eq!(
        outcome.status,
        TurnStatus::BudgetExhausted(BudgetReason::InputTokens)
    );
    assert!(bridge.requests().expect("request log").is_empty());
    assert_eq!(outcome.usage.model_calls, 0);
}

#[test]
fn product_runner_preserves_tool_order_correlation_and_committed_revision() {
    let executor = Arc::new(RecordingExecutor::default());
    let runner = AgentRunner::new(executor.clone());
    let bridge = MockBridge::scripted([
        MockResponse::completion(tool_response(vec![
            call("call-first", "commit_first"),
            call("call-second", "inspect_after"),
        ])),
        MockResponse::text("done"),
    ]);
    let cancellation = CancellationToken::new();

    let outcome = block_on(runner.run_turn(invocation(
        &bridge,
        &cancellation,
        ResourceBudget::default(),
    )));

    assert_eq!(outcome.status, TurnStatus::Completed);
    assert_eq!(outcome.final_text.as_deref(), Some("done"));
    assert_eq!(
        executor.calls(),
        vec![
            ("commit_first".to_owned(), Revision::new(1)),
            ("inspect_after".to_owned(), Revision::new(2)),
        ]
    );
    assert_eq!(outcome.tool_results[0].call_id.as_str(), "call-first");
    assert_eq!(outcome.tool_results[1].call_id.as_str(), "call-second");
    assert_eq!(outcome.committed_events.len(), 2);
    assert_eq!(
        outcome.committed_events[1].to_string(),
        "evt_01890f6a-2b41-7d4e-8f90-123456789abc"
    );
    let requests = bridge.requests().expect("mock requests");
    assert_eq!(requests.len(), 2);
    assert!(matches!(
        &requests[1].messages[1].content[..],
        [ContentPart::ToolCall(first), ContentPart::ToolCall(second)]
            if first.id.as_str() == "call-first" && second.id.as_str() == "call-second"
    ));
    assert!(matches!(
        &requests[1].messages[2].content[..],
        [ContentPart::ToolResult(result)] if result.call_id.as_str() == "call-first"
    ));
    assert!(matches!(
        &requests[1].messages[3].content[..],
        [ContentPart::ToolResult(result)] if result.call_id.as_str() == "call-second"
    ));
}

#[test]
fn committed_tool_is_retained_when_follow_up_model_budget_is_exhausted() {
    let executor = Arc::new(RecordingExecutor::default());
    let runner = AgentRunner::new(executor);
    let bridge = MockBridge::scripted([MockResponse::completion(tool_response(vec![call(
        "call-first",
        "commit_first",
    )]))]);
    let cancellation = CancellationToken::new();
    let budget = ResourceBudget {
        max_model_calls: 1,
        ..ResourceBudget::default()
    };

    let outcome = block_on(runner.run_turn(invocation(&bridge, &cancellation, budget)));

    assert_eq!(
        outcome.status,
        TurnStatus::BudgetExhausted(BudgetReason::ModelCalls)
    );
    assert_eq!(outcome.tool_results.len(), 1);
    assert_eq!(outcome.committed_events.len(), 2);
}

struct PendingBridge {
    cancellation: CancellationToken,
    dropped: Arc<AtomicBool>,
    calls: AtomicUsize,
}

struct DropProbe(Arc<AtomicBool>);

impl Drop for DropProbe {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

impl LlmBridge for PendingBridge {
    fn capabilities(&self) -> BridgeCapabilities {
        BridgeCapabilities::all()
    }

    fn project(&self, _request: &CompletionRequest) -> Result<ProjectionReport, BridgeError> {
        Ok(ProjectionReport::exact("pending-test"))
    }

    fn complete<'a>(
        &'a self,
        _request: CompletionRequest,
    ) -> BridgeFuture<'a, Result<CompletionResponse, BridgeError>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let _probe = DropProbe(Arc::clone(&self.dropped));
            poll_fn(|context| -> Poll<Result<CompletionResponse, BridgeError>> {
                self.cancellation.cancel();
                context.waker().wake_by_ref();
                Poll::Pending
            })
            .await
        })
    }

    fn stream<'a>(
        &'a self,
        _request: CompletionRequest,
    ) -> BridgeFuture<'a, Result<CompletionStream, BridgeError>> {
        Box::pin(async {
            Err(BridgeError::InvalidRequest {
                message: "streaming is not used".to_owned(),
            })
        })
    }
}

#[test]
fn cancellation_wakes_and_drops_an_in_flight_provider_call() {
    let cancellation = CancellationToken::new();
    let dropped = Arc::new(AtomicBool::new(false));
    let bridge = PendingBridge {
        cancellation: cancellation.clone(),
        dropped: Arc::clone(&dropped),
        calls: AtomicUsize::new(0),
    };
    let runner = AgentRunner::new(Arc::new(RecordingExecutor::default()));

    let outcome = block_on(runner.run_turn(invocation(
        &bridge,
        &cancellation,
        ResourceBudget::default(),
    )));

    assert_eq!(outcome.status, TurnStatus::Cancelled);
    assert_eq!(bridge.calls.load(Ordering::SeqCst), 1);
    assert!(dropped.load(Ordering::SeqCst));
    assert!(outcome.tool_results.is_empty());
}

#[test]
fn reset_rearms_the_same_shared_cancellation_identity() {
    let token = CancellationToken::new();
    let application_handle = token.clone();
    application_handle.cancel();
    assert!(token.is_cancelled());
    token.reset();
    assert!(!application_handle.is_cancelled());
    application_handle.cancel();
    assert!(token.is_cancelled());
}

#[test]
fn canonical_agent_wire_rejects_unknown_fields_and_invalid_plans() {
    let unknown = json!({
        "based_on_revision": 1,
        "npc_turns": [],
        "priority": 10
    });
    assert!(serde_json::from_value::<NarratorPlan>(unknown).is_err());

    let duplicate = json!({
        "based_on_revision": 1,
        "npc_turns": [
            {
                "request_id": "ntr_01890f6a-2b50-7d4e-8f90-123456789abc",
                "actor_id": "obj_01890f6a-2b51-7d4e-8f90-123456789abc",
                "scene_id": "obj_01890f6a-2b52-7d4e-8f90-123456789abc",
                "based_on_revision": 1,
                "assignment": "listen"
            },
            {
                "request_id": "ntr_01890f6a-2b50-7d4e-8f90-123456789abc",
                "actor_id": "obj_01890f6a-2b53-7d4e-8f90-123456789abc",
                "scene_id": "obj_01890f6a-2b52-7d4e-8f90-123456789abc",
                "based_on_revision": 1,
                "assignment": "answer"
            }
        ]
    });
    let duplicate = serde_json::from_value::<NarratorPlan>(duplicate).expect("wire shape");
    assert!(duplicate.validate().is_err());
}
