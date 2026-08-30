//! P0 evidence for Loreloom's explicit Agent and Narrator loops.
//!
//! Every Loreloom type in this file is deliberately test-only. Armillae owns
//! one model call and one ToolCall execution; this spike proves that Loreloom
//! can own all continuation, orchestration, revision, budget, and cancel logic.

use std::collections::{BTreeMap, BTreeSet};
use std::future::poll_fn;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::Poll;

use armillae_core::{
    AssistantContent, CompletionRequest, CompletionResponse, ContentPart, FinishReason, Message,
    TextContent, TokenUsage, ToolCall, ToolCallId, ToolDefinition, ToolResult, ToolResultContent,
};
use armillae_llm::{
    BoxFuture as BridgeFuture, BridgeCapabilities, BridgeError, CompletionStream, LlmBridge,
    MockBridge, MockResponse, ProjectionReport,
};
use armillae_tools::{BoxFuture as ToolFuture, ToolContext, ToolExecutionError, ToolExecutor};
use futures_executor::block_on;
use futures_util::future::{Either, select};
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};

const SCENE_ID: &str = "scene/old-mill";

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().expect("test mutex remains available")
}

fn call_id(value: &str) -> ToolCallId {
    ToolCallId::new(value).expect("fixture ToolCall ID is non-empty")
}

fn tool_call(id: &str, name: &str, arguments: JsonValue) -> ToolCall {
    ToolCall {
        id: call_id(id),
        name: name.to_owned(),
        arguments,
    }
}

fn text_response(text: impl Into<String>) -> CompletionResponse {
    CompletionResponse {
        id: None,
        model: Some("mock-loreloom".to_owned()),
        content: vec![AssistantContent::Text(TextContent::new(text))],
        finish_reason: Some(FinishReason::Stop),
        usage: None,
        provider_metadata: JsonValue::Null,
    }
}

fn text_response_with_usage(
    text: impl Into<String>,
    input_tokens: u64,
    output_tokens: u64,
) -> CompletionResponse {
    let mut response = text_response(text);
    response.usage = Some(TokenUsage {
        input_tokens: Some(input_tokens),
        output_tokens: Some(output_tokens),
        total_tokens: Some(input_tokens + output_tokens),
        cached_input_tokens: None,
    });
    response
}

fn tool_response(calls: Vec<ToolCall>) -> CompletionResponse {
    CompletionResponse {
        id: None,
        model: Some("mock-loreloom".to_owned()),
        content: calls.into_iter().map(AssistantContent::ToolCall).collect(),
        finish_reason: Some(FinishReason::ToolCall),
        usage: None,
        provider_metadata: JsonValue::Null,
    }
}

fn response_text(response: &CompletionResponse) -> String {
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

fn message_text(message: &Message) -> String {
    message
        .content
        .iter()
        .filter_map(|content| match content {
            ContentPart::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
enum BudgetReason {
    ModelCalls,
    ToolCalls,
    InputTokens,
    OutputTokens,
    TotalTokens,
    OutputBytes,
    Deadline,
    MissingTokenUsage,
    AgentTurns,
    OrchestrationRounds,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ResourceLimits {
    max_model_calls: u32,
    max_tool_calls: u32,
    max_input_tokens: u64,
    max_output_tokens: u64,
    max_total_tokens: u64,
    max_output_bytes: usize,
    max_elapsed_ms: u64,
    require_reported_tokens: bool,
}

impl ResourceLimits {
    const fn generous() -> Self {
        Self {
            max_model_calls: 32,
            max_tool_calls: 64,
            max_input_tokens: 1_000_000,
            max_output_tokens: 1_000_000,
            max_total_tokens: 2_000_000,
            max_output_bytes: 1_000_000,
            max_elapsed_ms: 60_000,
            require_reported_tokens: false,
        }
    }

    fn strictest(values: &[Self]) -> Self {
        values
            .iter()
            .copied()
            .reduce(|left, right| Self {
                max_model_calls: left.max_model_calls.min(right.max_model_calls),
                max_tool_calls: left.max_tool_calls.min(right.max_tool_calls),
                max_input_tokens: left.max_input_tokens.min(right.max_input_tokens),
                max_output_tokens: left.max_output_tokens.min(right.max_output_tokens),
                max_total_tokens: left.max_total_tokens.min(right.max_total_tokens),
                max_output_bytes: left.max_output_bytes.min(right.max_output_bytes),
                max_elapsed_ms: left.max_elapsed_ms.min(right.max_elapsed_ms),
                require_reported_tokens: left.require_reported_tokens
                    || right.require_reported_tokens,
            })
            .unwrap_or_else(Self::generous)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OrchestrationLimits {
    resources: ResourceLimits,
    max_agent_turns: u32,
    max_rounds: u32,
}

impl OrchestrationLimits {
    const fn generous() -> Self {
        Self {
            resources: ResourceLimits::generous(),
            max_agent_turns: 32,
            max_rounds: 8,
        }
    }

    fn strictest(values: &[Self]) -> Self {
        values
            .iter()
            .copied()
            .reduce(|left, right| Self {
                resources: ResourceLimits::strictest(&[left.resources, right.resources]),
                max_agent_turns: left.max_agent_turns.min(right.max_agent_turns),
                max_rounds: left.max_rounds.min(right.max_rounds),
            })
            .unwrap_or_else(Self::generous)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ResourceUsage {
    model_calls: u32,
    tool_calls: u32,
    input_tokens: u64,
    output_tokens: u64,
    total_tokens: u64,
    output_bytes: usize,
    missing_token_reports: u32,
}

impl ResourceUsage {
    fn before_model(
        &mut self,
        limits: ResourceLimits,
        elapsed_ms: u64,
    ) -> Result<(), BudgetReason> {
        self.check_deadline(limits, elapsed_ms)?;
        if self.model_calls >= limits.max_model_calls {
            return Err(BudgetReason::ModelCalls);
        }
        self.model_calls += 1;
        Ok(())
    }

    fn before_tool(&mut self, limits: ResourceLimits, elapsed_ms: u64) -> Result<(), BudgetReason> {
        self.check_deadline(limits, elapsed_ms)?;
        if self.tool_calls >= limits.max_tool_calls {
            return Err(BudgetReason::ToolCalls);
        }
        self.tool_calls += 1;
        Ok(())
    }

    fn after_response(
        &mut self,
        limits: ResourceLimits,
        response: &CompletionResponse,
        elapsed_ms: u64,
    ) -> Result<(), BudgetReason> {
        self.check_deadline(limits, elapsed_ms)?;
        self.output_bytes = self
            .output_bytes
            .saturating_add(response_text(response).len());
        if self.output_bytes > limits.max_output_bytes {
            return Err(BudgetReason::OutputBytes);
        }

        let Some(usage) = &response.usage else {
            self.missing_token_reports += 1;
            if limits.require_reported_tokens {
                return Err(BudgetReason::MissingTokenUsage);
            }
            return Ok(());
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
        if self.input_tokens > limits.max_input_tokens {
            return Err(BudgetReason::InputTokens);
        }
        if self.output_tokens > limits.max_output_tokens {
            return Err(BudgetReason::OutputTokens);
        }
        if self.total_tokens > limits.max_total_tokens {
            return Err(BudgetReason::TotalTokens);
        }
        Ok(())
    }

    fn check_deadline(&self, limits: ResourceLimits, elapsed_ms: u64) -> Result<(), BudgetReason> {
        if elapsed_ms > limits.max_elapsed_ms {
            Err(BudgetReason::Deadline)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct OrchestrationUsage {
    resources: ResourceUsage,
    agent_turns: u32,
    rounds: u32,
}

impl OrchestrationUsage {
    fn start_turn(&mut self, limits: OrchestrationLimits) -> Result<(), BudgetReason> {
        if self.agent_turns >= limits.max_agent_turns {
            return Err(BudgetReason::AgentTurns);
        }
        self.agent_turns += 1;
        Ok(())
    }

    fn start_round(&mut self, limits: OrchestrationLimits) -> Result<(), BudgetReason> {
        if self.rounds >= limits.max_rounds {
            return Err(BudgetReason::OrchestrationRounds);
        }
        self.rounds += 1;
        Ok(())
    }
}

#[derive(Default)]
struct FakeClock(AtomicU64);

impl FakeClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }

    fn advance_ms(&self, value: u64) {
        self.0.fetch_add(value, Ordering::SeqCst);
    }
}

#[derive(Default)]
struct SlotProbe {
    active: AtomicUsize,
    maximum: AtomicUsize,
    transitions: Mutex<Vec<String>>,
}

impl SlotProbe {
    fn enter(self: &Arc<Self>, label: &str) -> SlotGuard {
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.maximum.fetch_max(active, Ordering::SeqCst);
        lock(&self.transitions).push(format!("enter:{label}"));
        SlotGuard {
            probe: Arc::clone(self),
            label: label.to_owned(),
        }
    }

    fn maximum(&self) -> usize {
        self.maximum.load(Ordering::SeqCst)
    }

    fn transitions(&self) -> Vec<String> {
        lock(&self.transitions).clone()
    }
}

struct SlotGuard {
    probe: Arc<SlotProbe>,
    label: String,
}

impl Drop for SlotGuard {
    fn drop(&mut self) {
        lock(&self.probe.transitions).push(format!("exit:{}", self.label));
        self.probe.active.fetch_sub(1, Ordering::SeqCst);
    }
}

#[derive(Clone, Debug)]
struct RuntimeToolContext {
    actor_id: String,
    revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct CommittedEvent {
    id: String,
    revision: u64,
    actor_id: String,
    fact: String,
}

#[derive(Clone, Debug)]
struct ActorRecord {
    name: String,
    scene_id: String,
}

#[derive(Clone, Debug)]
struct WorldState {
    revision: u64,
    actors: BTreeMap<String, ActorRecord>,
    events: Vec<CommittedEvent>,
}

#[derive(Clone)]
struct WorldGateway {
    state: Arc<Mutex<WorldState>>,
}

impl WorldGateway {
    fn new() -> Self {
        let actors = [
            (
                "character/mira".to_owned(),
                ActorRecord {
                    name: "Mira".to_owned(),
                    scene_id: SCENE_ID.to_owned(),
                },
            ),
            (
                "character/tomas".to_owned(),
                ActorRecord {
                    name: "Tomas".to_owned(),
                    scene_id: SCENE_ID.to_owned(),
                },
            ),
        ]
        .into_iter()
        .collect();
        Self {
            state: Arc::new(Mutex::new(WorldState {
                revision: 0,
                actors,
                events: Vec::new(),
            })),
        }
    }

    fn revision(&self) -> u64 {
        lock(&self.state).revision
    }

    fn project(&self, request: &NpcTurnRequest) -> Result<NpcContext, NpcTurnStatus> {
        let state = lock(&self.state);
        let Some(actor) = state.actors.get(&request.actor_id) else {
            return Err(NpcTurnStatus::Stale);
        };
        if actor.scene_id != request.scene_id {
            return Err(NpcTurnStatus::Stale);
        }
        Ok(NpcContext {
            actor_id: request.actor_id.clone(),
            revision: state.revision,
            character: CharacterContext {
                actor_id: request.actor_id.clone(),
                name: actor.name.clone(),
                revision: state.revision,
            },
            scene: SceneContext {
                scene_id: actor.scene_id.clone(),
                summary: "Dusty mill floor and a sealed bell tower".to_owned(),
                revision: state.revision,
            },
            assignment: NpcAssignment {
                text: request.assignment.clone(),
                revision: state.revision,
            },
        })
    }

    fn events(&self) -> Vec<CommittedEvent> {
        lock(&self.state).events.clone()
    }
}

#[derive(Clone)]
struct RecordingToolExecutor {
    gateway: WorldGateway,
    calls: Arc<Mutex<Vec<(String, String, u64)>>>,
    cancel: Arc<AtomicBool>,
    cancel_after_calls: Option<usize>,
    clock: Arc<FakeClock>,
    advance_after_call_ms: u64,
}

impl RecordingToolExecutor {
    fn new(gateway: WorldGateway, cancel: Arc<AtomicBool>, clock: Arc<FakeClock>) -> Self {
        Self {
            gateway,
            calls: Arc::new(Mutex::new(Vec::new())),
            cancel,
            cancel_after_calls: None,
            clock,
            advance_after_call_ms: 0,
        }
    }

    fn with_cancel_after_calls(mut self, count: usize) -> Self {
        self.cancel_after_calls = Some(count);
        self
    }

    fn with_clock_advance(mut self, milliseconds: u64) -> Self {
        self.advance_after_call_ms = milliseconds;
        self
    }

    fn calls(&self) -> Vec<(String, String, u64)> {
        lock(&self.calls).clone()
    }

    fn result(call: &ToolCall, value: JsonValue, is_error: bool) -> ToolResult {
        ToolResult {
            call_id: call.id.clone(),
            content: vec![ToolResultContent::Json { value }],
            is_error,
        }
    }
}

impl ToolExecutor for RecordingToolExecutor {
    fn definitions(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "observe_scene".to_owned(),
                description: "Return the current authorized scene revision".to_owned(),
                input_schema: json!({ "type": "object" }),
            },
            ToolDefinition {
                name: "commit_event".to_owned(),
                description: "Commit one validated world event".to_owned(),
                input_schema: json!({
                    "type": "object",
                    "required": ["fact"],
                    "properties": { "fact": { "type": "string" } }
                }),
            },
        ]
    }

    fn execute<'a>(
        &'a self,
        context: ToolContext,
        call: ToolCall,
    ) -> ToolFuture<'a, Result<ToolResult, ToolExecutionError>> {
        Box::pin(async move {
            let runtime = context.get::<RuntimeToolContext>().ok_or_else(|| {
                ToolExecutionError::ExecutionFailed {
                    name: call.name.clone(),
                    message: "missing Loreloom ToolContext".to_owned(),
                }
            })?;
            let call_count = {
                let mut calls = lock(&self.calls);
                calls.push((
                    call.id.as_str().to_owned(),
                    call.name.clone(),
                    runtime.revision,
                ));
                calls.len()
            };

            let result = match call.name.as_str() {
                "observe_scene" => Self::result(
                    &call,
                    json!({
                        "revision": self.gateway.revision(),
                        "scene_id": SCENE_ID,
                    }),
                    false,
                ),
                "commit_event" => {
                    let fact = call
                        .arguments
                        .get("fact")
                        .and_then(JsonValue::as_str)
                        .ok_or_else(|| ToolExecutionError::InvalidArguments {
                            name: call.name.clone(),
                            message: "fact must be a string".to_owned(),
                        })?;
                    let mut world = lock(&self.gateway.state);
                    if world.revision != runtime.revision {
                        Self::result(
                            &call,
                            json!({
                                "code": "revision_conflict",
                                "expected_revision": runtime.revision,
                                "actual_revision": world.revision,
                            }),
                            true,
                        )
                    } else {
                        world.revision += 1;
                        let event = CommittedEvent {
                            id: format!("event/{}", world.revision),
                            revision: world.revision,
                            actor_id: runtime.actor_id.clone(),
                            fact: fact.to_owned(),
                        };
                        world.events.push(event.clone());
                        Self::result(
                            &call,
                            json!({
                                "event_id": event.id,
                                "revision": event.revision,
                                "fact": event.fact,
                            }),
                            false,
                        )
                    }
                }
                _ => {
                    return Err(ToolExecutionError::UnknownTool {
                        name: call.name.clone(),
                    });
                }
            };

            if self.advance_after_call_ms > 0 {
                self.clock.advance_ms(self.advance_after_call_ms);
            }
            if self.cancel_after_calls == Some(call_count) {
                self.cancel.store(true, Ordering::SeqCst);
            }
            Ok(result)
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum TurnStatus {
    Completed,
    Cancelled,
    BudgetExhausted(BudgetReason),
    Failed(String),
}

#[derive(Clone, Debug)]
struct TurnOutcome {
    status: TurnStatus,
    final_text: Option<String>,
    tool_results: Vec<ToolResult>,
    committed_events: Vec<String>,
    usage: ResourceUsage,
}

struct AgentRunner {
    executor: Arc<dyn ToolExecutor>,
    clock: Arc<FakeClock>,
    slot: Arc<SlotProbe>,
}

struct TurnInvocation<'a> {
    label: &'a str,
    bridge: &'a dyn LlmBridge,
    request: CompletionRequest,
    tool_context: RuntimeToolContext,
    turn_limits: ResourceLimits,
    orchestration_limits: ResourceLimits,
    cancel: &'a AtomicBool,
}

impl AgentRunner {
    async fn run_turn(
        &self,
        invocation: TurnInvocation<'_>,
        orchestration_usage: &mut ResourceUsage,
    ) -> TurnOutcome {
        let TurnInvocation {
            label,
            bridge,
            mut request,
            tool_context,
            turn_limits,
            orchestration_limits,
            cancel,
        } = invocation;
        let _slot = self.slot.enter(label);
        let started_at = self.clock.now_ms();
        let mut usage = ResourceUsage::default();
        let mut tool_results = Vec::new();
        let mut committed_events = Vec::new();

        loop {
            if cancel.load(Ordering::SeqCst) {
                return TurnOutcome {
                    status: TurnStatus::Cancelled,
                    final_text: None,
                    tool_results,
                    committed_events,
                    usage,
                };
            }
            let elapsed = self.clock.now_ms().saturating_sub(started_at);
            if let Err(reason) = usage.before_model(turn_limits, elapsed) {
                return budget_outcome(reason, usage, tool_results, committed_events);
            }
            if let Err(reason) = orchestration_usage.before_model(orchestration_limits, elapsed) {
                return budget_outcome(reason, usage, tool_results, committed_events);
            }

            if let Err(error) = bridge.project(&request) {
                return failed_outcome(error.to_string(), usage, tool_results, committed_events);
            }
            let model_call = bridge.complete(request.clone());
            let cancellation = Box::pin(poll_fn(|context| {
                if cancel.load(Ordering::SeqCst) {
                    Poll::Ready(())
                } else {
                    context.waker().wake_by_ref();
                    Poll::Pending
                }
            }));
            let response = match select(model_call, cancellation).await {
                Either::Left((Ok(response), _)) => response,
                Either::Left((Err(error), _)) => {
                    return failed_outcome(
                        error.to_string(),
                        usage,
                        tool_results,
                        committed_events,
                    );
                }
                Either::Right(((), pending_model_call)) => {
                    drop(pending_model_call);
                    return TurnOutcome {
                        status: TurnStatus::Cancelled,
                        final_text: None,
                        tool_results,
                        committed_events,
                        usage,
                    };
                }
            };
            let elapsed = self.clock.now_ms().saturating_sub(started_at);
            if let Err(reason) = usage.after_response(turn_limits, &response, elapsed) {
                return budget_outcome(reason, usage, tool_results, committed_events);
            }
            if let Err(reason) =
                orchestration_usage.after_response(orchestration_limits, &response, elapsed)
            {
                return budget_outcome(reason, usage, tool_results, committed_events);
            }

            let calls = response.tool_calls().cloned().collect::<Vec<_>>();
            let final_text = response_text(&response);
            request.messages.push(response.as_assistant_message());
            if calls.is_empty() {
                return TurnOutcome {
                    status: TurnStatus::Completed,
                    final_text: Some(final_text),
                    tool_results,
                    committed_events,
                    usage,
                };
            }

            for call in calls {
                if cancel.load(Ordering::SeqCst) {
                    return TurnOutcome {
                        status: TurnStatus::Cancelled,
                        final_text: None,
                        tool_results,
                        committed_events,
                        usage,
                    };
                }
                let elapsed = self.clock.now_ms().saturating_sub(started_at);
                if let Err(reason) = usage.before_tool(turn_limits, elapsed) {
                    return budget_outcome(reason, usage, tool_results, committed_events);
                }
                if let Err(reason) = orchestration_usage.before_tool(orchestration_limits, elapsed)
                {
                    return budget_outcome(reason, usage, tool_results, committed_events);
                }
                let context = ToolContext::new().with_extension(tool_context.clone());
                let result = match self.executor.execute(context, call.clone()).await {
                    Ok(result) => result,
                    Err(error) => ToolResult {
                        call_id: call.id.clone(),
                        content: vec![ToolResultContent::Json {
                            value: json!({
                                "code": "tool_execution_error",
                                "message": error.to_string(),
                            }),
                        }],
                        is_error: true,
                    },
                };
                if !result.is_error {
                    for content in &result.content {
                        if let ToolResultContent::Json { value } = content
                            && let Some(event_id) =
                                value.get("event_id").and_then(JsonValue::as_str)
                        {
                            committed_events.push(event_id.to_owned());
                        }
                    }
                }
                request.messages.push(Message::tool_result(result.clone()));
                tool_results.push(result);
            }
        }
    }
}

fn budget_outcome(
    reason: BudgetReason,
    usage: ResourceUsage,
    tool_results: Vec<ToolResult>,
    committed_events: Vec<String>,
) -> TurnOutcome {
    TurnOutcome {
        status: TurnStatus::BudgetExhausted(reason),
        final_text: None,
        tool_results,
        committed_events,
        usage,
    }
}

fn failed_outcome(
    message: String,
    usage: ResourceUsage,
    tool_results: Vec<ToolResult>,
    committed_events: Vec<String>,
) -> TurnOutcome {
    TurnOutcome {
        status: TurnStatus::Failed(message),
        final_text: None,
        tool_results,
        committed_events,
        usage,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct NarratorPlan {
    based_on_revision: u64,
    npc_turns: Vec<NpcTurnRequest>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct NpcTurnRequest {
    request_id: String,
    actor_id: String,
    scene_id: String,
    based_on_revision: u64,
    assignment: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct CharacterContext {
    actor_id: String,
    name: String,
    revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct SceneContext {
    scene_id: String,
    summary: String,
    revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct NpcAssignment {
    text: String,
    revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct NpcContext {
    actor_id: String,
    revision: u64,
    character: CharacterContext,
    scene: SceneContext,
    assignment: NpcAssignment,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AgentDefinition {
    profile_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct NpcAgent {
    definition: AgentDefinition,
    context: NpcContext,
}

impl NpcAgent {
    fn new(
        definition: AgentDefinition,
        character: CharacterContext,
        scene: SceneContext,
        assignment: NpcAssignment,
    ) -> Self {
        let revision = character.revision;
        assert_eq!(scene.revision, revision);
        assert_eq!(assignment.revision, revision);
        Self {
            context: NpcContext {
                actor_id: character.actor_id.clone(),
                revision,
                character,
                scene,
                assignment,
            },
            definition,
        }
    }

    fn request(&self, definitions: Vec<ToolDefinition>) -> CompletionRequest {
        CompletionRequest {
            messages: vec![Message::user(
                serde_json::to_string(&json!({
                    "kind": "npc_turn",
                    "profile_id": self.definition.profile_id,
                    "context": self.context,
                }))
                .expect("test NPC context serializes"),
            )],
            tools: definitions,
            ..CompletionRequest::default()
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
enum NpcTurnStatus {
    Completed,
    Stale,
    Cancelled,
    BudgetExhausted(BudgetReason),
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct NpcTurnResult {
    request_id: String,
    actor_id: String,
    observed_revision: Option<u64>,
    final_revision: u64,
    status: NpcTurnStatus,
    utterance: Option<String>,
    intent: Option<String>,
    claimed_action_description: Option<String>,
    tool_call_ids: Vec<String>,
    world_events: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct NpcModelOutput {
    utterance: Option<String>,
    intent: Option<String>,
    claimed_action_description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SynthesisEnvelope {
    Final {
        based_on_revision: u64,
        narration: String,
        supporting_events: Vec<String>,
    },
    Continue {
        based_on_revision: u64,
        narration: Option<String>,
        supporting_events: Vec<String>,
        next_plan: NarratorPlan,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct SynthesisNpcView {
    request_id: String,
    actor_id: String,
    status: NpcTurnStatus,
    utterance: Option<String>,
    intent: Option<String>,
    claimed_action_description: Option<String>,
    committed_event_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum OrchestrationStatus {
    Completed,
    Cancelled,
    BudgetExhausted(BudgetReason),
    Failed(String),
}

#[derive(Clone, Debug)]
struct OrchestrationOutcome {
    status: OrchestrationStatus,
    narration: Option<String>,
    npc_results: Vec<NpcTurnResult>,
    usage: OrchestrationUsage,
}

struct Orchestrator {
    gateway: WorldGateway,
    runner: AgentRunner,
    turn_limits: ResourceLimits,
    orchestration_limits: OrchestrationLimits,
    cancel: Arc<AtomicBool>,
}

impl Orchestrator {
    async fn run(
        &self,
        player_input: &str,
        narrator: &MockBridge,
        npc_bridges: &BTreeMap<String, Arc<MockBridge>>,
    ) -> OrchestrationOutcome {
        let mut usage = OrchestrationUsage::default();
        if let Err(reason) = usage.start_round(self.orchestration_limits) {
            return orchestration_budget(reason, usage, Vec::new());
        }
        if let Err(reason) = usage.start_turn(self.orchestration_limits) {
            return orchestration_budget(reason, usage, Vec::new());
        }

        let planning = self
            .runner
            .run_turn(
                TurnInvocation {
                    label: "narrator:planning",
                    bridge: narrator,
                    request: CompletionRequest {
                        messages: vec![Message::user(player_input)],
                        ..CompletionRequest::default()
                    },
                    tool_context: RuntimeToolContext {
                        actor_id: "agent/narrator".to_owned(),
                        revision: self.gateway.revision(),
                    },
                    turn_limits: self.turn_limits,
                    orchestration_limits: self.orchestration_limits.resources,
                    cancel: &self.cancel,
                },
                &mut usage.resources,
            )
            .await;
        let Some(plan_text) = planning.final_text else {
            return outcome_from_turn(planning.status, usage, Vec::new());
        };
        let mut plan: NarratorPlan = match serde_json::from_str(&plan_text) {
            Ok(plan) => plan,
            Err(error) => {
                return OrchestrationOutcome {
                    status: OrchestrationStatus::Failed(format!("invalid NarratorPlan: {error}")),
                    narration: None,
                    npc_results: Vec::new(),
                    usage,
                };
            }
        };
        if plan.based_on_revision != self.gateway.revision() {
            return OrchestrationOutcome {
                status: OrchestrationStatus::Failed("stale NarratorPlan".to_owned()),
                narration: None,
                npc_results: Vec::new(),
                usage,
            };
        }

        let mut npc_results = Vec::new();
        loop {
            let mut round_budget_failure = None;
            for request in &plan.npc_turns {
                if self.cancel.load(Ordering::SeqCst) {
                    npc_results.push(unstarted_result(
                        request,
                        self.gateway.revision(),
                        NpcTurnStatus::Cancelled,
                    ));
                    continue;
                }
                if let Some(reason) = round_budget_failure {
                    npc_results.push(unstarted_result(
                        request,
                        self.gateway.revision(),
                        NpcTurnStatus::BudgetExhausted(reason),
                    ));
                    continue;
                }
                let projected = match self.gateway.project(request) {
                    Ok(context) => context,
                    Err(status) => {
                        npc_results.push(unstarted_result(
                            request,
                            self.gateway.revision(),
                            status,
                        ));
                        continue;
                    }
                };
                let Some(bridge) = npc_bridges.get(&request.actor_id) else {
                    npc_results.push(unstarted_result(
                        request,
                        self.gateway.revision(),
                        NpcTurnStatus::Failed,
                    ));
                    continue;
                };
                if let Err(reason) = usage.start_turn(self.orchestration_limits) {
                    round_budget_failure = Some(reason);
                    npc_results.push(unstarted_result(
                        request,
                        self.gateway.revision(),
                        NpcTurnStatus::BudgetExhausted(reason),
                    ));
                    continue;
                }

                let observed_revision = projected.revision;
                let npc = NpcAgent::new(
                    AgentDefinition {
                        profile_id: format!("profile/{}", request.actor_id),
                    },
                    projected.character,
                    projected.scene,
                    projected.assignment,
                );
                let turn_label = format!("npc:{}", request.actor_id);
                let turn = self
                    .runner
                    .run_turn(
                        TurnInvocation {
                            label: &turn_label,
                            bridge: bridge.as_ref(),
                            request: npc.request(self.runner.executor.definitions()),
                            tool_context: RuntimeToolContext {
                                actor_id: request.actor_id.clone(),
                                revision: observed_revision,
                            },
                            turn_limits: self.turn_limits,
                            orchestration_limits: self.orchestration_limits.resources,
                            cancel: &self.cancel,
                        },
                        &mut usage.resources,
                    )
                    .await;
                let status = match turn.status {
                    TurnStatus::Completed => NpcTurnStatus::Completed,
                    TurnStatus::Cancelled => NpcTurnStatus::Cancelled,
                    TurnStatus::BudgetExhausted(reason) => {
                        round_budget_failure = Some(reason);
                        NpcTurnStatus::BudgetExhausted(reason)
                    }
                    TurnStatus::Failed(_) => NpcTurnStatus::Failed,
                };
                let model_output = turn
                    .final_text
                    .as_deref()
                    .and_then(|text| serde_json::from_str::<NpcModelOutput>(text).ok())
                    .unwrap_or_default();
                npc_results.push(NpcTurnResult {
                    request_id: request.request_id.clone(),
                    actor_id: request.actor_id.clone(),
                    observed_revision: Some(observed_revision),
                    final_revision: self.gateway.revision(),
                    status,
                    utterance: model_output.utterance,
                    intent: model_output.intent,
                    claimed_action_description: model_output.claimed_action_description,
                    tool_call_ids: turn
                        .tool_results
                        .iter()
                        .map(|result| result.call_id.as_str().to_owned())
                        .collect(),
                    world_events: turn.committed_events,
                });
            }

            if self.cancel.load(Ordering::SeqCst) {
                return OrchestrationOutcome {
                    status: OrchestrationStatus::Cancelled,
                    narration: None,
                    npc_results,
                    usage,
                };
            }
            if let Some(reason) = round_budget_failure {
                return orchestration_budget(reason, usage, npc_results);
            }
            if let Err(reason) = usage.start_turn(self.orchestration_limits) {
                return orchestration_budget(reason, usage, npc_results);
            }

            let committed_events = self.gateway.events();
            let npc_views = npc_results
                .iter()
                .map(|result| SynthesisNpcView {
                    request_id: result.request_id.clone(),
                    actor_id: result.actor_id.clone(),
                    status: result.status.clone(),
                    utterance: result.utterance.clone(),
                    intent: result.intent.clone(),
                    claimed_action_description: result.claimed_action_description.clone(),
                    committed_event_ids: result.world_events.clone(),
                })
                .collect::<Vec<_>>();
            let synthesis_request = CompletionRequest {
                messages: vec![Message::user(
                    serde_json::to_string(&json!({
                        "kind": "narrator_synthesis",
                        "revision": self.gateway.revision(),
                        "npc_outputs_are_claims": true,
                        "npc_results": npc_views,
                        "committed_events": committed_events,
                    }))
                    .expect("test synthesis context serializes"),
                )],
                ..CompletionRequest::default()
            };
            let synthesis = self
                .runner
                .run_turn(
                    TurnInvocation {
                        label: "narrator:synthesis",
                        bridge: narrator,
                        request: synthesis_request,
                        tool_context: RuntimeToolContext {
                            actor_id: "agent/narrator".to_owned(),
                            revision: self.gateway.revision(),
                        },
                        turn_limits: self.turn_limits,
                        orchestration_limits: self.orchestration_limits.resources,
                        cancel: &self.cancel,
                    },
                    &mut usage.resources,
                )
                .await;
            let Some(synthesis_text) = synthesis.final_text else {
                return outcome_from_turn(synthesis.status, usage, npc_results);
            };
            let envelope: SynthesisEnvelope = match serde_json::from_str(&synthesis_text) {
                Ok(envelope) => envelope,
                Err(error) => {
                    return OrchestrationOutcome {
                        status: OrchestrationStatus::Failed(format!(
                            "invalid NarratorSynthesis: {error}"
                        )),
                        narration: None,
                        npc_results,
                        usage,
                    };
                }
            };
            let committed_ids = self
                .gateway
                .events()
                .into_iter()
                .map(|event| event.id)
                .collect::<BTreeSet<_>>();
            let supporting_events = match &envelope {
                SynthesisEnvelope::Final {
                    supporting_events, ..
                }
                | SynthesisEnvelope::Continue {
                    supporting_events, ..
                } => supporting_events,
            };
            if supporting_events
                .iter()
                .any(|event| !committed_ids.contains(event))
            {
                return OrchestrationOutcome {
                    status: OrchestrationStatus::Failed(
                        "NarratorSynthesis referenced an uncommitted event".to_owned(),
                    ),
                    narration: None,
                    npc_results,
                    usage,
                };
            }

            match envelope {
                SynthesisEnvelope::Final {
                    based_on_revision,
                    narration,
                    ..
                } => {
                    if based_on_revision != self.gateway.revision() {
                        return OrchestrationOutcome {
                            status: OrchestrationStatus::Failed(
                                "stale NarratorSynthesis".to_owned(),
                            ),
                            narration: None,
                            npc_results,
                            usage,
                        };
                    }
                    return OrchestrationOutcome {
                        status: OrchestrationStatus::Completed,
                        narration: Some(narration),
                        npc_results,
                        usage,
                    };
                }
                SynthesisEnvelope::Continue {
                    based_on_revision,
                    next_plan,
                    ..
                } => {
                    if based_on_revision != self.gateway.revision()
                        || next_plan.based_on_revision != self.gateway.revision()
                    {
                        return OrchestrationOutcome {
                            status: OrchestrationStatus::Failed(
                                "stale continuation plan".to_owned(),
                            ),
                            narration: None,
                            npc_results,
                            usage,
                        };
                    }
                    if let Err(reason) = usage.start_round(self.orchestration_limits) {
                        return orchestration_budget(reason, usage, npc_results);
                    }
                    plan = next_plan;
                }
            }
        }
    }
}

fn unstarted_result(
    request: &NpcTurnRequest,
    revision: u64,
    status: NpcTurnStatus,
) -> NpcTurnResult {
    NpcTurnResult {
        request_id: request.request_id.clone(),
        actor_id: request.actor_id.clone(),
        observed_revision: None,
        final_revision: revision,
        status,
        utterance: None,
        intent: None,
        claimed_action_description: None,
        tool_call_ids: Vec::new(),
        world_events: Vec::new(),
    }
}

fn orchestration_budget(
    reason: BudgetReason,
    usage: OrchestrationUsage,
    npc_results: Vec<NpcTurnResult>,
) -> OrchestrationOutcome {
    OrchestrationOutcome {
        status: OrchestrationStatus::BudgetExhausted(reason),
        narration: None,
        npc_results,
        usage,
    }
}

fn outcome_from_turn(
    status: TurnStatus,
    usage: OrchestrationUsage,
    npc_results: Vec<NpcTurnResult>,
) -> OrchestrationOutcome {
    let status = match status {
        TurnStatus::Completed => {
            OrchestrationStatus::Failed("turn completed without required output".to_owned())
        }
        TurnStatus::Cancelled => OrchestrationStatus::Cancelled,
        TurnStatus::BudgetExhausted(reason) => OrchestrationStatus::BudgetExhausted(reason),
        TurnStatus::Failed(message) => OrchestrationStatus::Failed(message),
    };
    OrchestrationOutcome {
        status,
        narration: None,
        npc_results,
        usage,
    }
}

fn runner_fixture(
    gateway: WorldGateway,
    executor: Arc<dyn ToolExecutor>,
    clock: Arc<FakeClock>,
    slot: Arc<SlotProbe>,
) -> AgentRunner {
    let _ = gateway;
    AgentRunner {
        executor,
        clock,
        slot,
    }
}

fn plan(requests: Vec<NpcTurnRequest>, revision: u64) -> NarratorPlan {
    NarratorPlan {
        based_on_revision: revision,
        npc_turns: requests,
    }
}

fn npc_request(id: &str, actor_id: &str, assignment: &str, revision: u64) -> NpcTurnRequest {
    NpcTurnRequest {
        request_id: id.to_owned(),
        actor_id: actor_id.to_owned(),
        scene_id: SCENE_ID.to_owned(),
        based_on_revision: revision,
        assignment: assignment.to_owned(),
    }
}

fn mock_text(value: impl Serialize) -> MockResponse {
    MockResponse::completion(text_response(
        serde_json::to_string(&value).expect("fixture serializes"),
    ))
}

struct PendingBridge {
    cancel: Arc<AtomicBool>,
    dropped: Arc<AtomicBool>,
    calls: AtomicUsize,
}

impl PendingBridge {
    fn new(cancel: Arc<AtomicBool>, dropped: Arc<AtomicBool>) -> Self {
        Self {
            cancel,
            dropped,
            calls: AtomicUsize::new(0),
        }
    }
}

struct PendingCallDrop(Arc<AtomicBool>);

impl Drop for PendingCallDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

impl LlmBridge for PendingBridge {
    fn capabilities(&self) -> BridgeCapabilities {
        BridgeCapabilities::all()
    }

    fn project(&self, _request: &CompletionRequest) -> Result<ProjectionReport, BridgeError> {
        Ok(ProjectionReport::exact("pending-spike"))
    }

    fn complete<'a>(
        &'a self,
        _request: CompletionRequest,
    ) -> BridgeFuture<'a, Result<CompletionResponse, BridgeError>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let _drop_probe = PendingCallDrop(Arc::clone(&self.dropped));
            poll_fn(|context| -> Poll<Result<CompletionResponse, BridgeError>> {
                self.cancel.store(true, Ordering::SeqCst);
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
                message: "pending spike uses complete only".to_owned(),
            })
        })
    }
}

#[test]
fn armillae_multi_tool_calls_are_serial_and_canonically_correlated() {
    let gateway = WorldGateway::new();
    let cancel = Arc::new(AtomicBool::new(false));
    let clock = Arc::new(FakeClock::default());
    let executor = Arc::new(RecordingToolExecutor::new(
        gateway.clone(),
        Arc::clone(&cancel),
        Arc::clone(&clock),
    ));
    let slot = Arc::new(SlotProbe::default());
    let runner = runner_fixture(
        gateway,
        executor.clone(),
        Arc::clone(&clock),
        Arc::clone(&slot),
    );
    let bridge = MockBridge::scripted([
        MockResponse::completion(tool_response(vec![
            tool_call("call-observe", "observe_scene", json!({})),
            tool_call("call-unknown", "not_registered", json!({})),
        ])),
        MockResponse::text("done"),
    ]);
    let mut outer_usage = ResourceUsage::default();
    let outcome = block_on(runner.run_turn(
        TurnInvocation {
            label: "npc:mira",
            bridge: &bridge,
            request: CompletionRequest {
                messages: vec![Message::user("inspect then try the named action")],
                tools: executor.definitions(),
                ..CompletionRequest::default()
            },
            tool_context: RuntimeToolContext {
                actor_id: "character/mira".to_owned(),
                revision: 0,
            },
            turn_limits: ResourceLimits::generous(),
            orchestration_limits: ResourceLimits::generous(),
            cancel: &cancel,
        },
        &mut outer_usage,
    ));

    assert_eq!(outcome.status, TurnStatus::Completed);
    assert_eq!(outcome.final_text.as_deref(), Some("done"));
    assert_eq!(
        executor.calls(),
        vec![
            ("call-observe".to_owned(), "observe_scene".to_owned(), 0),
            ("call-unknown".to_owned(), "not_registered".to_owned(), 0),
        ]
    );
    assert_eq!(outcome.tool_results.len(), 2);
    assert_eq!(outcome.tool_results[0].call_id.as_str(), "call-observe");
    assert!(!outcome.tool_results[0].is_error);
    assert_eq!(outcome.tool_results[1].call_id.as_str(), "call-unknown");
    assert!(outcome.tool_results[1].is_error);

    let requests = bridge.requests().expect("Mock Bridge requests can be read");
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].messages.len(), 4);
    assert!(matches!(
        &requests[1].messages[1].content[..],
        [ContentPart::ToolCall(first), ContentPart::ToolCall(second)]
            if first.id.as_str() == "call-observe" && second.id.as_str() == "call-unknown"
    ));
    assert!(matches!(
        &requests[1].messages[2].content[..],
        [ContentPart::ToolResult(result)] if result.call_id.as_str() == "call-observe"
    ));
    assert!(matches!(
        &requests[1].messages[3].content[..],
        [ContentPart::ToolResult(result)] if result.call_id.as_str() == "call-unknown"
    ));
    assert_eq!(outcome.usage.model_calls, 2);
    assert_eq!(outcome.usage.tool_calls, 2);
    assert_eq!(slot.maximum(), 1);
}

#[test]
fn pending_model_call_is_dropped_when_runtime_cancel_wins_the_race() {
    let gateway = WorldGateway::new();
    let cancel = Arc::new(AtomicBool::new(false));
    let dropped = Arc::new(AtomicBool::new(false));
    let clock = Arc::new(FakeClock::default());
    let executor = Arc::new(RecordingToolExecutor::new(
        gateway.clone(),
        Arc::clone(&cancel),
        Arc::clone(&clock),
    ));
    let runner = runner_fixture(gateway, executor, clock, Arc::new(SlotProbe::default()));
    let bridge = PendingBridge::new(Arc::clone(&cancel), Arc::clone(&dropped));
    let mut outer_usage = ResourceUsage::default();
    let outcome = block_on(runner.run_turn(
        TurnInvocation {
            label: "npc:pending-provider",
            bridge: &bridge,
            request: CompletionRequest {
                messages: vec![Message::user("wait for provider")],
                ..CompletionRequest::default()
            },
            tool_context: RuntimeToolContext {
                actor_id: "character/mira".to_owned(),
                revision: 0,
            },
            turn_limits: ResourceLimits::generous(),
            orchestration_limits: ResourceLimits::generous(),
            cancel: &cancel,
        },
        &mut outer_usage,
    ));

    assert_eq!(outcome.status, TurnStatus::Cancelled);
    assert_eq!(bridge.calls.load(Ordering::SeqCst), 1);
    assert!(dropped.load(Ordering::SeqCst));
    assert!(outcome.tool_results.is_empty());
}

#[test]
fn narrator_plan_reprojects_each_npc_and_serializes_synthesis_after_commits() {
    let gateway = WorldGateway::new();
    let cancel = Arc::new(AtomicBool::new(false));
    let clock = Arc::new(FakeClock::default());
    let executor = Arc::new(RecordingToolExecutor::new(
        gateway.clone(),
        Arc::clone(&cancel),
        Arc::clone(&clock),
    ));
    let slot = Arc::new(SlotProbe::default());
    let runner = runner_fixture(
        gateway.clone(),
        executor.clone(),
        Arc::clone(&clock),
        Arc::clone(&slot),
    );
    let narrator_plan = plan(
        vec![
            npc_request("npc-request/1", "character/mira", "Ring the bell", 0),
            npc_request("npc-request/2", "character/tomas", "React to the bell", 0),
            npc_request(
                "npc-request/3",
                "character/missing",
                "Speak from nowhere",
                0,
            ),
        ],
        0,
    );
    let narrator = MockBridge::scripted([
        mock_text(&narrator_plan),
        mock_text(SynthesisEnvelope::Final {
            based_on_revision: 1,
            narration: "Mira pulls the rope; the committed bell answers.".to_owned(),
            supporting_events: vec!["event/1".to_owned()],
        }),
    ]);
    let mira = Arc::new(MockBridge::scripted([
        MockResponse::tool_call(
            call_id("mira-call/1"),
            "commit_event",
            json!({ "fact": "Mira rang the mill bell" }),
        ),
        mock_text(NpcModelOutput {
            utterance: Some("Wake, old mill.".to_owned()),
            intent: Some("alert the yard".to_owned()),
            claimed_action_description: Some("pulled the bell rope".to_owned()),
        }),
    ]));
    let tomas = Arc::new(MockBridge::scripted([mock_text(NpcModelOutput {
        utterance: Some("I heard it.".to_owned()),
        intent: Some("find Mira".to_owned()),
        claimed_action_description: None,
    })]));
    let missing = Arc::new(MockBridge::scripted([MockResponse::text(
        "must never be called",
    )]));
    let npc_bridges = BTreeMap::from([
        ("character/mira".to_owned(), Arc::clone(&mira)),
        ("character/tomas".to_owned(), Arc::clone(&tomas)),
        ("character/missing".to_owned(), Arc::clone(&missing)),
    ]);
    let orchestrator = Orchestrator {
        gateway: gateway.clone(),
        runner,
        turn_limits: ResourceLimits::generous(),
        orchestration_limits: OrchestrationLimits::generous(),
        cancel,
    };

    let outcome = block_on(orchestrator.run(
        "Ask Mira to ring the bell, then see who reacts.",
        &narrator,
        &npc_bridges,
    ));
    assert_eq!(outcome.status, OrchestrationStatus::Completed);
    assert_eq!(gateway.revision(), 1);
    assert_eq!(outcome.npc_results.len(), 3);
    assert_eq!(outcome.npc_results[0].request_id, "npc-request/1");
    assert_eq!(outcome.npc_results[0].observed_revision, Some(0));
    assert_eq!(outcome.npc_results[0].world_events, vec!["event/1"]);
    assert_eq!(outcome.npc_results[1].request_id, "npc-request/2");
    assert_eq!(outcome.npc_results[1].observed_revision, Some(1));
    assert_eq!(outcome.npc_results[2].status, NpcTurnStatus::Stale);
    assert_eq!(outcome.npc_results[2].observed_revision, None);
    assert_eq!(
        missing
            .requests()
            .expect("missing NPC bridge request log is readable")
            .len(),
        0
    );

    let tomas_requests = tomas.requests().expect("Tomas request log is readable");
    let tomas_context: JsonValue = serde_json::from_str(&message_text(
        tomas_requests[0]
            .messages
            .first()
            .expect("NPC request has context"),
    ))
    .expect("NPC context is JSON");
    assert_eq!(tomas_context["context"]["revision"], 1);
    assert_eq!(tomas_context["context"]["character"]["revision"], 1);
    assert_eq!(tomas_context["context"]["scene"]["revision"], 1);
    assert_eq!(tomas_context["context"]["assignment"]["revision"], 1);
    assert!(!message_text(&tomas_requests[0].messages[0]).contains("Ask Mira to ring the bell"));

    let narrator_requests = narrator.requests().expect("Narrator log is readable");
    assert_eq!(narrator_requests.len(), 2);
    assert!(message_text(&narrator_requests[0].messages[0]).contains("Ask Mira to ring the bell"));
    let synthesis: JsonValue =
        serde_json::from_str(&message_text(&narrator_requests[1].messages[0]))
            .expect("synthesis context is JSON");
    assert_eq!(synthesis["npc_outputs_are_claims"], true);
    assert_eq!(synthesis["committed_events"][0]["id"], "event/1");
    assert_eq!(
        synthesis["npc_results"][0]["claimed_action_description"],
        "pulled the bell rope"
    );
    assert_eq!(
        outcome.narration.as_deref(),
        Some("Mira pulls the rope; the committed bell answers.")
    );
    assert_eq!(slot.maximum(), 1);
    assert_eq!(
        slot.transitions(),
        vec![
            "enter:narrator:planning",
            "exit:narrator:planning",
            "enter:npc:character/mira",
            "exit:npc:character/mira",
            "enter:npc:character/tomas",
            "exit:npc:character/tomas",
            "enter:narrator:synthesis",
            "exit:narrator:synthesis",
        ]
    );
}

#[test]
fn synthesis_rejects_a_claim_that_has_no_committed_event_basis() {
    let gateway = WorldGateway::new();
    let cancel = Arc::new(AtomicBool::new(false));
    let clock = Arc::new(FakeClock::default());
    let executor = Arc::new(RecordingToolExecutor::new(
        gateway.clone(),
        Arc::clone(&cancel),
        Arc::clone(&clock),
    ));
    let runner = runner_fixture(
        gateway.clone(),
        executor,
        clock,
        Arc::new(SlotProbe::default()),
    );
    let narrator = MockBridge::scripted([
        mock_text(plan(
            vec![npc_request(
                "npc-request/claim",
                "character/mira",
                "Try the sealed vault",
                0,
            )],
            0,
        )),
        mock_text(SynthesisEnvelope::Final {
            based_on_revision: 0,
            narration: "The vault is now open.".to_owned(),
            supporting_events: vec!["event/fabricated".to_owned()],
        }),
    ]);
    let mira = Arc::new(MockBridge::scripted([mock_text(NpcModelOutput {
        utterance: Some("It is open.".to_owned()),
        intent: None,
        claimed_action_description: Some("opened the vault".to_owned()),
    })]));
    let orchestrator = Orchestrator {
        gateway: gateway.clone(),
        runner,
        turn_limits: ResourceLimits::generous(),
        orchestration_limits: OrchestrationLimits::generous(),
        cancel,
    };
    let outcome = block_on(orchestrator.run(
        "Open the vault.",
        &narrator,
        &BTreeMap::from([("character/mira".to_owned(), mira)]),
    ));

    assert_eq!(
        outcome.status,
        OrchestrationStatus::Failed("NarratorSynthesis referenced an uncommitted event".to_owned())
    );
    assert_eq!(gateway.revision(), 0);
    assert!(gateway.events().is_empty());
    assert_eq!(outcome.npc_results[0].world_events, Vec::<String>::new());
}

#[test]
fn cancellation_keeps_committed_tools_and_correlates_every_accepted_request() {
    let gateway = WorldGateway::new();
    let cancel = Arc::new(AtomicBool::new(false));
    let clock = Arc::new(FakeClock::default());
    let executor = Arc::new(
        RecordingToolExecutor::new(gateway.clone(), Arc::clone(&cancel), Arc::clone(&clock))
            .with_cancel_after_calls(1),
    );
    let runner = runner_fixture(
        gateway.clone(),
        executor.clone(),
        clock,
        Arc::new(SlotProbe::default()),
    );
    let narrator = MockBridge::scripted([mock_text(plan(
        vec![
            npc_request("request/1", "character/mira", "Ring once", 0),
            npc_request("request/2", "character/tomas", "Answer", 0),
            npc_request("request/3", "character/mira", "Ring twice", 0),
        ],
        0,
    ))]);
    let mira = Arc::new(MockBridge::scripted([MockResponse::completion(
        tool_response(vec![
            tool_call(
                "commit-before-cancel",
                "commit_event",
                json!({ "fact": "The bell rang once" }),
            ),
            tool_call("must-not-run", "observe_scene", json!({})),
        ]),
    )]));
    let tomas = Arc::new(MockBridge::scripted([MockResponse::text("must not start")]));
    let npc_bridges = BTreeMap::from([
        ("character/mira".to_owned(), mira),
        ("character/tomas".to_owned(), Arc::clone(&tomas)),
    ]);
    let orchestrator = Orchestrator {
        gateway: gateway.clone(),
        runner,
        turn_limits: ResourceLimits::generous(),
        orchestration_limits: OrchestrationLimits::generous(),
        cancel,
    };
    let outcome = block_on(orchestrator.run("Ring and answer.", &narrator, &npc_bridges));

    assert_eq!(outcome.status, OrchestrationStatus::Cancelled);
    assert_eq!(gateway.revision(), 1);
    assert_eq!(gateway.events()[0].id, "event/1");
    assert_eq!(
        executor.calls(),
        vec![(
            "commit-before-cancel".to_owned(),
            "commit_event".to_owned(),
            0,
        )]
    );
    assert_eq!(outcome.npc_results.len(), 3);
    assert_eq!(outcome.npc_results[0].request_id, "request/1");
    assert_eq!(outcome.npc_results[0].status, NpcTurnStatus::Cancelled);
    assert_eq!(outcome.npc_results[0].world_events, vec!["event/1"]);
    assert_eq!(outcome.npc_results[1].request_id, "request/2");
    assert_eq!(outcome.npc_results[1].status, NpcTurnStatus::Cancelled);
    assert_eq!(outcome.npc_results[2].request_id, "request/3");
    assert_eq!(outcome.npc_results[2].status, NpcTurnStatus::Cancelled);
    assert!(
        tomas
            .requests()
            .expect("Tomas request log is readable")
            .is_empty()
    );
}

#[test]
fn configurable_turn_and_orchestration_budgets_fail_independently() {
    let global = OrchestrationLimits {
        resources: ResourceLimits {
            max_model_calls: 12,
            max_tool_calls: 9,
            max_input_tokens: 1_000,
            max_output_tokens: 500,
            max_total_tokens: 1_500,
            max_output_bytes: 8_000,
            max_elapsed_ms: 5_000,
            require_reported_tokens: false,
        },
        max_agent_turns: 8,
        max_rounds: 4,
    };
    let save = OrchestrationLimits {
        resources: ResourceLimits {
            max_model_calls: 10,
            max_tool_calls: 8,
            max_input_tokens: 900,
            max_output_tokens: 400,
            max_total_tokens: 1_200,
            max_output_bytes: 7_000,
            max_elapsed_ms: 4_000,
            require_reported_tokens: false,
        },
        max_agent_turns: 6,
        max_rounds: 3,
    };
    let profile = OrchestrationLimits {
        resources: ResourceLimits {
            max_model_calls: 7,
            max_tool_calls: 6,
            max_input_tokens: 800,
            max_output_tokens: 350,
            max_total_tokens: 1_000,
            max_output_bytes: 6_000,
            max_elapsed_ms: 3_000,
            require_reported_tokens: true,
        },
        max_agent_turns: 5,
        max_rounds: 2,
    };
    let effective = OrchestrationLimits::strictest(&[global, save, profile]);
    assert_eq!(effective, profile);

    let gateway = WorldGateway::new();
    let cancel = Arc::new(AtomicBool::new(false));
    let clock = Arc::new(FakeClock::default());
    let executor = Arc::new(RecordingToolExecutor::new(
        gateway.clone(),
        Arc::clone(&cancel),
        Arc::clone(&clock),
    ));
    let runner = runner_fixture(
        gateway,
        executor.clone(),
        clock,
        Arc::new(SlotProbe::default()),
    );
    let bridge = MockBridge::scripted([MockResponse::completion(tool_response(vec![
        tool_call("budget-call/1", "observe_scene", json!({})),
        tool_call("budget-call/2", "observe_scene", json!({})),
    ]))]);
    let mut outer_usage = ResourceUsage::default();
    let turn_limits = ResourceLimits {
        max_tool_calls: 1,
        ..ResourceLimits::generous()
    };
    let outcome = block_on(runner.run_turn(
        TurnInvocation {
            label: "npc:budget",
            bridge: &bridge,
            request: CompletionRequest {
                messages: vec![Message::user("two tools")],
                tools: executor.definitions(),
                ..CompletionRequest::default()
            },
            tool_context: RuntimeToolContext {
                actor_id: "character/mira".to_owned(),
                revision: 0,
            },
            turn_limits,
            orchestration_limits: ResourceLimits::generous(),
            cancel: &cancel,
        },
        &mut outer_usage,
    ));
    assert_eq!(
        outcome.status,
        TurnStatus::BudgetExhausted(BudgetReason::ToolCalls)
    );
    assert_eq!(executor.calls().len(), 1);
    assert_eq!(outcome.tool_results.len(), 1);

    let gateway = WorldGateway::new();
    let cancel = Arc::new(AtomicBool::new(false));
    let clock = Arc::new(FakeClock::default());
    let executor = Arc::new(RecordingToolExecutor::new(
        gateway.clone(),
        Arc::clone(&cancel),
        Arc::clone(&clock),
    ));
    let runner = runner_fixture(
        gateway.clone(),
        executor,
        clock,
        Arc::new(SlotProbe::default()),
    );
    let narrator = MockBridge::scripted([mock_text(plan(
        vec![npc_request("outer-budget/1", "character/mira", "Speak", 0)],
        0,
    ))]);
    let mira = Arc::new(MockBridge::scripted([MockResponse::text(
        "must not be called",
    )]));
    let orchestrator = Orchestrator {
        gateway,
        runner,
        turn_limits: ResourceLimits::generous(),
        orchestration_limits: OrchestrationLimits {
            resources: ResourceLimits {
                max_model_calls: 1,
                ..ResourceLimits::generous()
            },
            ..OrchestrationLimits::generous()
        },
        cancel,
    };
    let outcome = block_on(orchestrator.run(
        "one total model call",
        &narrator,
        &BTreeMap::from([("character/mira".to_owned(), Arc::clone(&mira))]),
    ));
    assert_eq!(
        outcome.status,
        OrchestrationStatus::BudgetExhausted(BudgetReason::ModelCalls)
    );
    assert_eq!(outcome.npc_results.len(), 1);
    assert_eq!(
        outcome.npc_results[0].status,
        NpcTurnStatus::BudgetExhausted(BudgetReason::ModelCalls)
    );
    assert!(
        mira.requests()
            .expect("Mira request log is readable")
            .is_empty()
    );
    assert_eq!(outcome.usage.resources.model_calls, 1);
}

#[test]
fn token_output_and_fake_clock_limits_never_treat_unknown_usage_as_zero() {
    let gateway = WorldGateway::new();
    let cancel = Arc::new(AtomicBool::new(false));
    let clock = Arc::new(FakeClock::default());
    let executor = Arc::new(RecordingToolExecutor::new(
        gateway.clone(),
        Arc::clone(&cancel),
        Arc::clone(&clock),
    ));
    let runner = runner_fixture(
        gateway,
        executor,
        Arc::clone(&clock),
        Arc::new(SlotProbe::default()),
    );
    let unknown_usage = MockBridge::scripted([MockResponse::text("unmetered")]);
    let mut outer_usage = ResourceUsage::default();
    let outcome = block_on(runner.run_turn(
        TurnInvocation {
            label: "npc:unknown-usage",
            bridge: &unknown_usage,
            request: CompletionRequest {
                messages: vec![Message::user("answer")],
                ..CompletionRequest::default()
            },
            tool_context: RuntimeToolContext {
                actor_id: "character/mira".to_owned(),
                revision: 0,
            },
            turn_limits: ResourceLimits {
                require_reported_tokens: true,
                ..ResourceLimits::generous()
            },
            orchestration_limits: ResourceLimits::generous(),
            cancel: &cancel,
        },
        &mut outer_usage,
    ));
    assert_eq!(
        outcome.status,
        TurnStatus::BudgetExhausted(BudgetReason::MissingTokenUsage)
    );
    assert_eq!(outcome.usage.missing_token_reports, 1);

    let reported = MockBridge::scripted([MockResponse::completion(text_response_with_usage(
        "12345", 8, 5,
    ))]);
    let mut outer_usage = ResourceUsage::default();
    let outcome = block_on(runner.run_turn(
        TurnInvocation {
            label: "npc:reported-usage",
            bridge: &reported,
            request: CompletionRequest {
                messages: vec![Message::user("answer")],
                ..CompletionRequest::default()
            },
            tool_context: RuntimeToolContext {
                actor_id: "character/mira".to_owned(),
                revision: 0,
            },
            turn_limits: ResourceLimits {
                max_output_tokens: 4,
                ..ResourceLimits::generous()
            },
            orchestration_limits: ResourceLimits::generous(),
            cancel: &cancel,
        },
        &mut outer_usage,
    ));
    assert_eq!(
        outcome.status,
        TurnStatus::BudgetExhausted(BudgetReason::OutputTokens)
    );

    let gateway = WorldGateway::new();
    let executor = Arc::new(
        RecordingToolExecutor::new(gateway, Arc::clone(&cancel), Arc::clone(&clock))
            .with_clock_advance(51),
    );
    let runner = AgentRunner {
        executor: executor.clone(),
        clock,
        slot: Arc::new(SlotProbe::default()),
    };
    let deadline = MockBridge::scripted([MockResponse::completion(tool_response(vec![
        tool_call("deadline/1", "observe_scene", json!({})),
        tool_call("deadline/2", "observe_scene", json!({})),
    ]))]);
    let mut outer_usage = ResourceUsage::default();
    let outcome = block_on(runner.run_turn(
        TurnInvocation {
            label: "npc:deadline",
            bridge: &deadline,
            request: CompletionRequest {
                messages: vec![Message::user("two timed tools")],
                tools: executor.definitions(),
                ..CompletionRequest::default()
            },
            tool_context: RuntimeToolContext {
                actor_id: "character/mira".to_owned(),
                revision: 0,
            },
            turn_limits: ResourceLimits {
                max_elapsed_ms: 50,
                ..ResourceLimits::generous()
            },
            orchestration_limits: ResourceLimits::generous(),
            cancel: &cancel,
        },
        &mut outer_usage,
    ));
    assert_eq!(
        outcome.status,
        TurnStatus::BudgetExhausted(BudgetReason::Deadline)
    );
    assert_eq!(executor.calls().len(), 1);
}

#[test]
fn continuation_obeys_configured_round_limit_without_fixed_npc_count() {
    let gateway = WorldGateway::new();
    let cancel = Arc::new(AtomicBool::new(false));
    let clock = Arc::new(FakeClock::default());
    let executor = Arc::new(RecordingToolExecutor::new(
        gateway.clone(),
        Arc::clone(&cancel),
        Arc::clone(&clock),
    ));
    let runner = runner_fixture(
        gateway.clone(),
        executor,
        clock,
        Arc::new(SlotProbe::default()),
    );
    let first_plan = plan(Vec::new(), 0);
    let narrator = MockBridge::scripted([
        mock_text(&first_plan),
        mock_text(SynthesisEnvelope::Continue {
            based_on_revision: 0,
            narration: Some("The room waits.".to_owned()),
            supporting_events: Vec::new(),
            next_plan: plan(
                vec![
                    npc_request("next/1", "character/mira", "Listen", 0),
                    npc_request("next/2", "character/tomas", "Watch", 0),
                ],
                0,
            ),
        }),
    ]);
    let orchestrator = Orchestrator {
        gateway,
        runner,
        turn_limits: ResourceLimits::generous(),
        orchestration_limits: OrchestrationLimits {
            max_rounds: 1,
            ..OrchestrationLimits::generous()
        },
        cancel,
    };
    let outcome = block_on(orchestrator.run("Wait.", &narrator, &BTreeMap::new()));

    assert_eq!(
        outcome.status,
        OrchestrationStatus::BudgetExhausted(BudgetReason::OrchestrationRounds)
    );
    assert_eq!(outcome.usage.rounds, 1);
    assert!(outcome.npc_results.is_empty());
}
