use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Instant,
};

use armillae_core::{CompletionRequest, ContentPart, Message, Role};
use armillae_llm::LlmBridge;
use loreloom_agent::{
    AgentDefinition, AgentRunner, AgentToolContext, BudgetReason, CancellationToken, NarratorPlan,
    NarratorSynthesis, NpcAgent, NpcAssignment, NpcModelOutput, NpcTurnRequest, NpcTurnResult,
    NpcTurnStatus, ResourceUsage, ToolCallOutcome, TurnInvocation, TurnOutcome, TurnStatus,
};
use loreloom_core::{
    ActorId, LongText, RuntimePhase, SessionId, ToolActivity, ToolActivityState, TranscriptSpeaker,
    UiSnapshot,
};
use serde_json::json;

use crate::{OrchestrationBudget, RuntimeConfig, RuntimeError, RuntimeToolExecutor, WorldService};

struct NpcRegistration {
    definition: AgentDefinition,
    bridge: Arc<dyn LlmBridge>,
}

pub struct GameRuntime {
    service: Arc<WorldService>,
    runner: AgentRunner,
    narrator: Arc<dyn LlmBridge>,
    npcs: BTreeMap<ActorId, NpcRegistration>,
    session_id: SessionId,
    config: RuntimeConfig,
    cancellation: CancellationToken,
}

impl std::fmt::Debug for GameRuntime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GameRuntime")
            .field("session_id", &self.session_id)
            .field("npc_count", &self.npcs.len())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlayerTurnOutcome {
    pub narration: loreloom_agent::NarrationText,
    pub npc_results: Vec<NpcTurnResult>,
    pub usage: ResourceUsage,
    pub snapshot: UiSnapshot,
}

impl GameRuntime {
    #[must_use]
    pub fn new(
        service: Arc<WorldService>,
        narrator: Arc<dyn LlmBridge>,
        session_id: SessionId,
        config: RuntimeConfig,
    ) -> Self {
        let executor = Arc::new(RuntimeToolExecutor::new(Arc::clone(&service)));
        Self {
            service,
            runner: AgentRunner::new(executor),
            narrator,
            npcs: BTreeMap::new(),
            session_id,
            config,
            cancellation: CancellationToken::new(),
        }
    }

    pub fn register_npc(
        &mut self,
        actor_id: ActorId,
        definition: AgentDefinition,
        bridge: Arc<dyn LlmBridge>,
    ) {
        self.npcs
            .insert(actor_id, NpcRegistration { definition, bridge });
    }

    #[must_use]
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation.clone()
    }

    pub async fn initial_snapshot(&self) -> Result<UiSnapshot, RuntimeError> {
        self.service
            .snapshot(
                self.session_id,
                RuntimePhase::Idle,
                Vec::new(),
                Vec::new(),
                Vec::new(),
            )
            .await
    }

    pub fn handle_player_input(
        &mut self,
        input: impl Into<String>,
    ) -> impl Future<Output = Result<PlayerTurnOutcome, RuntimeError>> + '_ {
        let input = input.into();
        let input = if input.trim().is_empty() {
            Err(RuntimeError::InvalidInput)
        } else {
            LongText::new(input).map_err(RuntimeError::from)
        };
        if input.is_ok() {
            self.cancellation.reset();
        }
        async move {
            let input = input?;
            self.handle_valid_player_input(input).await
        }
    }

    async fn handle_valid_player_input(
        &mut self,
        input: LongText,
    ) -> Result<PlayerTurnOutcome, RuntimeError> {
        let started = Instant::now();
        let mut orchestration = OrchestrationState::default();
        let mut tool_activity = Vec::new();
        let mut npc_results = Vec::new();

        let before = self
            .service
            .observation(self.session_id, input.clone())
            .await?;
        self.service
            .append_transcript(
                before.player.actor_id,
                self.session_id,
                TranscriptSpeaker::Player {
                    actor_id: before.player.actor_id,
                    display_name: before.player.display_name.clone(),
                },
                input.clone(),
                Vec::new(),
            )
            .await?;

        orchestration.start_round(self.config.orchestration_budget, started)?;
        orchestration.start_turn(self.config.orchestration_budget, started)?;
        let observation = self
            .service
            .observation(self.session_id, input.clone())
            .await?;
        let planning_request = narrator_request(
            "narrator_planning",
            json!({
                "observation": observation,
                "output_contract": "NarratorPlan"
            }),
            self.runner.definitions(),
        )?;
        let planning = self
            .runner
            .run_turn(TurnInvocation {
                bridge: self.narrator.as_ref(),
                request: planning_request,
                tool_context: AgentToolContext {
                    actor_id: before.player.actor_id,
                    revision: self.service.revision().await,
                    session_id: self.session_id,
                    capabilities: self.config.narrator_capabilities.clone(),
                },
                budget: self.config.turn_budget,
                cancellation: &self.cancellation,
            })
            .await;
        append_tool_activity(&mut tool_activity, &planning.tool_calls);
        orchestration.merge(&planning.usage, self.config.orchestration_budget, started)?;
        let planning_text = require_text(&planning)?;
        let mut plan: NarratorPlan =
            serde_json::from_str(&planning_text).map_err(|_| RuntimeError::ModelProtocol {
                stage: "narrator_plan",
            })?;
        plan.validate()?;
        if plan.based_on_revision != self.service.revision().await {
            return Err(RuntimeError::ModelProtocol {
                stage: "stale_narrator_plan",
            });
        }

        loop {
            let mut budget_failure = None;
            for request in &plan.npc_turns {
                if self.cancellation.is_cancelled() {
                    npc_results.push(unstarted_result(
                        request,
                        self.service.revision().await,
                        NpcTurnStatus::Cancelled,
                    ));
                    continue;
                }
                if let Some(reason) = budget_failure {
                    npc_results.push(unstarted_result(
                        request,
                        self.service.revision().await,
                        NpcTurnStatus::BudgetExhausted(reason),
                    ));
                    continue;
                }
                let Some(registration) = self.npcs.get(&request.actor_id) else {
                    npc_results.push(unstarted_result(
                        request,
                        self.service.revision().await,
                        NpcTurnStatus::Rejected,
                    ));
                    continue;
                };
                if self.service.agent_profile(request.actor_id).await
                    != Some(registration.definition.profile_id.clone())
                {
                    npc_results.push(unstarted_result(
                        request,
                        self.service.revision().await,
                        NpcTurnStatus::Stale,
                    ));
                    continue;
                }
                let (character, scene, dialogue) = match self
                    .service
                    .npc_context(request.actor_id, request.scene_id)
                    .await
                {
                    Ok(context) => context,
                    Err(_) => {
                        npc_results.push(unstarted_result(
                            request,
                            self.service.revision().await,
                            NpcTurnStatus::Stale,
                        ));
                        continue;
                    }
                };
                if let Err(error) =
                    orchestration.start_turn(self.config.orchestration_budget, started)
                {
                    let RuntimeError::Budget(reason) = error else {
                        return Err(error);
                    };
                    budget_failure = Some(reason);
                    npc_results.push(unstarted_result(
                        request,
                        self.service.revision().await,
                        NpcTurnStatus::BudgetExhausted(reason),
                    ));
                    continue;
                }
                let observed_revision = character.revision;
                let agent = NpcAgent::new(
                    registration.definition.clone(),
                    character,
                    scene,
                    NpcAssignment {
                        text: request.assignment.clone(),
                        revision: observed_revision,
                    },
                    dialogue,
                )?;
                let npc_request = agent.request(self.runner.definitions())?;
                let turn = self
                    .runner
                    .run_turn(TurnInvocation {
                        bridge: registration.bridge.as_ref(),
                        request: npc_request,
                        tool_context: AgentToolContext {
                            actor_id: request.actor_id,
                            revision: observed_revision,
                            session_id: self.session_id,
                            capabilities: registration.definition.allowed_tools.clone(),
                        },
                        budget: self.config.turn_budget,
                        cancellation: &self.cancellation,
                    })
                    .await;
                append_tool_activity(&mut tool_activity, &turn.tool_calls);
                if let Err(reason) = orchestration.merge_reason(
                    &turn.usage,
                    self.config.orchestration_budget,
                    started,
                ) {
                    budget_failure = Some(reason);
                }
                let (status, output) = npc_output(&turn);
                if let NpcTurnStatus::BudgetExhausted(reason) = status {
                    budget_failure = Some(reason);
                }
                npc_results.push(NpcTurnResult {
                    request_id: request.request_id,
                    actor_id: request.actor_id,
                    observed_revision: Some(observed_revision),
                    final_revision: self.service.revision().await,
                    status,
                    utterance: output.utterance,
                    intent: output.intent,
                    claimed_action_description: output.claimed_action_description,
                    tool_call_ids: turn
                        .tool_calls
                        .iter()
                        .map(|tool| tool.call_id.clone())
                        .collect(),
                    world_events: turn.committed_events,
                });
            }

            if self.cancellation.is_cancelled() {
                return Err(RuntimeError::Cancelled);
            }
            if let Some(reason) = budget_failure {
                return Err(RuntimeError::Budget(reason));
            }
            orchestration.start_turn(self.config.orchestration_budget, started)?;
            let revision = self.service.revision().await;
            let committed_events = self.service.events().await;
            let synthesis_request = narrator_request(
                "narrator_synthesis",
                json!({
                    "revision": revision,
                    "npc_outputs_are_claims": true,
                    "npc_results": npc_results,
                    "committed_events": committed_events,
                    "output_contract": "NarratorSynthesis"
                }),
                self.runner.definitions(),
            )?;
            let synthesis = self
                .runner
                .run_turn(TurnInvocation {
                    bridge: self.narrator.as_ref(),
                    request: synthesis_request,
                    tool_context: AgentToolContext {
                        actor_id: before.player.actor_id,
                        revision,
                        session_id: self.session_id,
                        capabilities: self.config.narrator_capabilities.clone(),
                    },
                    budget: self.config.turn_budget,
                    cancellation: &self.cancellation,
                })
                .await;
            append_tool_activity(&mut tool_activity, &synthesis.tool_calls);
            orchestration.merge(&synthesis.usage, self.config.orchestration_budget, started)?;
            let synthesis_text = require_text(&synthesis)?;
            let envelope: NarratorSynthesis =
                serde_json::from_str(&synthesis_text).map_err(|_| RuntimeError::ModelProtocol {
                    stage: "narrator_synthesis",
                })?;
            let committed_ids = self
                .service
                .events()
                .await
                .into_iter()
                .map(|event| event.id)
                .collect::<BTreeSet<_>>();
            let supporting = match &envelope {
                NarratorSynthesis::Final {
                    supporting_events, ..
                }
                | NarratorSynthesis::Continue {
                    supporting_events, ..
                } => supporting_events,
            };
            if supporting
                .iter()
                .any(|event| !committed_ids.contains(event))
            {
                return Err(RuntimeError::ModelProtocol {
                    stage: "uncommitted_supporting_event",
                });
            }
            match envelope {
                NarratorSynthesis::Final {
                    based_on_revision,
                    narration,
                    supporting_events,
                } => {
                    if based_on_revision != self.service.revision().await {
                        return Err(RuntimeError::ModelProtocol {
                            stage: "stale_synthesis",
                        });
                    }
                    self.service
                        .append_transcript(
                            before.player.actor_id,
                            self.session_id,
                            TranscriptSpeaker::Narrator,
                            narration.clone(),
                            supporting_events.clone(),
                        )
                        .await?;
                    let snapshot = self
                        .service
                        .snapshot(
                            self.session_id,
                            RuntimePhase::Completed,
                            tool_activity,
                            Vec::new(),
                            supporting_events,
                        )
                        .await?;
                    return Ok(PlayerTurnOutcome {
                        narration,
                        npc_results,
                        usage: orchestration.usage,
                        snapshot,
                    });
                }
                NarratorSynthesis::Continue {
                    based_on_revision,
                    next_plan,
                    ..
                } => {
                    if based_on_revision != self.service.revision().await
                        || next_plan.based_on_revision != based_on_revision
                    {
                        return Err(RuntimeError::ModelProtocol {
                            stage: "stale_continuation",
                        });
                    }
                    orchestration.start_round(self.config.orchestration_budget, started)?;
                    next_plan.validate()?;
                    plan = next_plan;
                }
            }
        }
    }
}

#[derive(Default)]
struct OrchestrationState {
    usage: ResourceUsage,
    started_turns: u32,
    rounds: u32,
}

impl OrchestrationState {
    fn start_turn(
        &mut self,
        budget: OrchestrationBudget,
        started: Instant,
    ) -> Result<(), RuntimeError> {
        self.check_deadline(budget, started)?;
        if self.started_turns >= budget.max_started_agent_turns {
            return Err(RuntimeError::Budget(BudgetReason::AgentTurns));
        }
        self.started_turns += 1;
        Ok(())
    }

    fn start_round(
        &mut self,
        budget: OrchestrationBudget,
        started: Instant,
    ) -> Result<(), RuntimeError> {
        self.check_deadline(budget, started)?;
        if self.rounds >= budget.max_orchestration_rounds {
            return Err(RuntimeError::Budget(BudgetReason::OrchestrationRounds));
        }
        self.rounds += 1;
        Ok(())
    }

    fn merge(
        &mut self,
        usage: &ResourceUsage,
        budget: OrchestrationBudget,
        started: Instant,
    ) -> Result<(), RuntimeError> {
        self.merge_reason(usage, budget, started)
            .map_err(RuntimeError::Budget)
    }

    fn merge_reason(
        &mut self,
        usage: &ResourceUsage,
        budget: OrchestrationBudget,
        started: Instant,
    ) -> Result<(), BudgetReason> {
        if elapsed_ms(started) > budget.resources.max_elapsed_ms {
            return Err(BudgetReason::Deadline);
        }
        self.usage.merge(usage, budget.resources)
    }

    fn check_deadline(
        &self,
        budget: OrchestrationBudget,
        started: Instant,
    ) -> Result<(), RuntimeError> {
        if elapsed_ms(started) > budget.resources.max_elapsed_ms {
            Err(RuntimeError::Budget(BudgetReason::Deadline))
        } else {
            Ok(())
        }
    }
}

fn narrator_request(
    kind: &'static str,
    payload: serde_json::Value,
    tools: Vec<armillae_core::ToolDefinition>,
) -> Result<CompletionRequest, RuntimeError> {
    let payload = serde_json::to_string(&json!({ "kind": kind, "payload": payload }))
        .map_err(|error| RuntimeError::json("narrator_context", error))?;
    Ok(CompletionRequest {
        messages: vec![
            Message::new(
                Role::System,
                vec![ContentPart::text(
                    "Player input goes only to the narrator. Use tools for world changes. NPC claims are not committed facts.",
                )],
            ),
            Message::user(payload),
        ],
        tools,
        ..CompletionRequest::default()
    })
}

fn require_text(outcome: &TurnOutcome) -> Result<String, RuntimeError> {
    match outcome.status {
        TurnStatus::Completed => outcome
            .final_text
            .clone()
            .ok_or(RuntimeError::ModelProtocol {
                stage: "missing_text",
            }),
        TurnStatus::Cancelled => Err(RuntimeError::Cancelled),
        TurnStatus::BudgetExhausted(reason) => Err(RuntimeError::Budget(reason)),
        TurnStatus::Failed(_) => Err(RuntimeError::BridgeUnavailable),
    }
}

fn npc_output(outcome: &TurnOutcome) -> (NpcTurnStatus, NpcModelOutput) {
    match outcome.status {
        TurnStatus::Completed => match outcome
            .final_text
            .as_deref()
            .and_then(|text| serde_json::from_str(text).ok())
        {
            Some(output) => (NpcTurnStatus::Completed, output),
            None => (NpcTurnStatus::Failed, NpcModelOutput::default()),
        },
        TurnStatus::Cancelled => (NpcTurnStatus::Cancelled, NpcModelOutput::default()),
        TurnStatus::BudgetExhausted(reason) => (
            NpcTurnStatus::BudgetExhausted(reason),
            NpcModelOutput::default(),
        ),
        TurnStatus::Failed(_) => (NpcTurnStatus::Failed, NpcModelOutput::default()),
    }
}

fn unstarted_result(
    request: &NpcTurnRequest,
    revision: loreloom_core::Revision,
    status: NpcTurnStatus,
) -> NpcTurnResult {
    NpcTurnResult {
        request_id: request.request_id,
        actor_id: request.actor_id,
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

fn append_tool_activity(activity: &mut Vec<ToolActivity>, tools: &[ToolCallOutcome]) {
    activity.extend(tools.iter().map(|tool| ToolActivity {
        call_id: tool.call_id.clone(),
        name: tool.name.clone(),
        state: if tool.is_error {
            ToolActivityState::Rejected
        } else {
            ToolActivityState::Succeeded
        },
    }));
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}
