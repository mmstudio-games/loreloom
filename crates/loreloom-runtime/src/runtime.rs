use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Instant,
};

use armillae_core::{CompletionRequest, ContentPart, Message, Role};
use armillae_llm::LlmBridge;
use loreloom_agent::{
    AgentDefinition, AgentRunner, AgentToolContext, BudgetReason, CancellationToken,
    ModelFailureDiagnostic, ModelInvocationKind, NarratorNpcDecision, NarratorPlan, NpcAgent,
    NpcAssignment, NpcControllerKind, NpcLifetime, NpcNarrativeAction, NpcTarget, NpcTurnRequest,
    NpcTurnResult, NpcTurnStatus, ResourceUsage, ToolCallOutcome, TurnInvocation, TurnOutcome,
    TurnStatus,
};
use loreloom_core::{
    ActorId, CharacterController, CharacterLifetime, ContentDefinitionId, GeneratedOrigin,
    GenerationId, GenerationSource, LongText, NoticeKind, Revision, RuntimePhase, SessionId,
    ShortText, ToolActivity, ToolActivityState, TranscriptItemId, TranscriptSpeaker, UiNotice,
    UiSnapshot, WorldCommandKind,
};
use serde::Serialize;
use serde_json::json;

use crate::{
    NARRATOR_SUBMIT_NPC_DRAFT_CAPABILITY, OrchestrationBudget, RuntimeConfig, RuntimeError,
    RuntimeToolExecutor, WorldService,
    context::{project_npc_context, project_observation},
    world_service::{CharacterMaterializationRequest, PendingNpcDecision},
};

struct NpcRegistration {
    definition: AgentDefinition,
    bridge: Arc<dyn LlmBridge>,
}

pub struct GameRuntime {
    service: Arc<WorldService>,
    executor: Arc<RuntimeToolExecutor>,
    runner: AgentRunner,
    narrator: Arc<dyn LlmBridge>,
    default_npc_bridge: Arc<dyn LlmBridge>,
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
            executor: Arc::clone(&executor),
            runner: AgentRunner::new(executor),
            narrator: Arc::clone(&narrator),
            default_npc_bridge: narrator,
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

    pub fn set_default_npc_bridge(&mut self, bridge: Arc<dyn LlmBridge>) {
        self.default_npc_bridge = bridge;
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
        self.handle_player_input_with_phase(input, |_| {})
    }

    pub fn handle_player_input_with_phase<'a, F>(
        &'a mut self,
        input: impl Into<String>,
        mut on_phase: F,
    ) -> impl Future<Output = Result<PlayerTurnOutcome, RuntimeError>> + 'a
    where
        F: FnMut(RuntimePhase) + 'a,
    {
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
            self.handle_valid_player_input(input, &mut on_phase).await
        }
    }

    async fn handle_valid_player_input<F>(
        &mut self,
        input: LongText,
        on_phase: &mut F,
    ) -> Result<PlayerTurnOutcome, RuntimeError>
    where
        F: FnMut(RuntimePhase),
    {
        self.config
            .context_projection
            .validate()
            .map_err(|field| RuntimeError::InvalidConfiguration { field })?;
        let started = Instant::now();
        let mut orchestration = OrchestrationState::default();
        let mut tool_activity = Vec::new();
        let mut npc_results = Vec::new();
        let mut notices = Vec::new();
        let _ = self.executor.take_pending_npc_decisions().await;
        let _ = self.executor.take_pending_npc_turns().await;
        let _ = self.executor.take_pending_scene_transition().await;

        let before = self
            .service
            .observation(self.session_id, input.clone())
            .await?;
        on_phase(RuntimePhase::PersistingInput);
        let player_transcript = self
            .service
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

        let mut materialization_results = Vec::new();
        let mut scene_transition_results = Vec::new();
        let mut settled_materializations = Vec::new();
        let mut generated_attempts = 0_u32;
        loop {
            on_phase(RuntimePhase::NarratorThinking);
            orchestration.start_round(self.config.orchestration_budget, started)?;
            orchestration.start_turn(self.config.orchestration_budget, started)?;
            let mut observation = self
                .service
                .observation(self.session_id, input.clone())
                .await?;
            project_observation(&mut observation, self.config.context_projection);
            let narrator_request = narrator_request(
                "narrator_turn",
                json!({
                    "observation": observation,
                    "materialization_results": materialization_results,
                    "scene_transition_results": scene_transition_results,
                    "npc_results": npc_results,
                    "committed_events": self.service.events().await,
                    "instructions": "Use native tools for structured decisions and world changes. request_npc_turn queues NPCs in tool-call order. If no more orchestration is needed, return only the final natural-language narration for the player. Do not return JSON or a structured envelope. NPC responses are claims; only committed events are world facts."
                }),
                self.runner
                    .definitions()
                    .into_iter()
                    .filter(|definition| definition.name != "submit_npc_draft")
                    .collect(),
            )?;
            let narrator_turn = self
                .runner
                .run_turn(TurnInvocation {
                    model_invocation: ModelInvocationKind::Narrator,
                    bridge: self.narrator.as_ref(),
                    request: narrator_request,
                    tool_context: AgentToolContext {
                        actor_id: before.player.actor_id,
                        revision: self.service.revision().await,
                        session_id: self.session_id,
                        capabilities: self.config.narrator_capabilities.clone(),
                    },
                    budget: self.config.turn_budget,
                    max_context_tokens: self.config.context_projection.max_context_tokens,
                    cancellation: &self.cancellation,
                })
                .await;
            append_tool_activity(&mut tool_activity, &narrator_turn.tool_calls);
            orchestration.merge(
                &narrator_turn.usage,
                self.config.orchestration_budget,
                started,
            )?;
            let pending = self.executor.take_pending_npc_decisions().await;
            let mut pending_turns = self.executor.take_pending_npc_turns().await;
            let pending_transition = self.executor.take_pending_scene_transition().await;
            let narrator_text = require_text(&narrator_turn)?;
            on_phase(RuntimePhase::ResolvingOrchestration);
            if let Some(pending_transition) = pending_transition {
                let context = AgentToolContext {
                    actor_id: before.player.actor_id,
                    revision: pending_transition.revision,
                    session_id: self.session_id,
                    capabilities: self.config.narrator_capabilities.clone(),
                };
                let target = pending_transition.target.clone();
                on_phase(RuntimePhase::UpdatingWorld);
                let result = self
                    .service
                    .execute(
                        &context,
                        WorldCommandKind::TransitionScene {
                            target: pending_transition.target,
                        },
                    )
                    .await;
                match result {
                    Ok(committed) => scene_transition_results.push(json!({
                        "call_id": pending_transition.call_id,
                        "target": target,
                        "status": "committed",
                        "scene_id": self.service.active_scene().await,
                        "revision": committed.revision
                    })),
                    Err(error @ (RuntimeError::World(_) | RuntimeError::Content(_))) => {
                        scene_transition_results.push(json!({
                            "call_id": pending_transition.call_id,
                            "target": target,
                            "status": "rejected",
                            "code": error.code(),
                            "revision": self.service.revision().await
                        }));
                    }
                    Err(error) => return Err(error),
                }
                pending_turns.clear();
                continue;
            }
            let requires_replanning = pending.iter().any(|pending| {
                pending.decision.requires_materialization()
                    && !settled_materializations.iter().any(
                        |(decision, _): &(NarratorNpcDecision, ActorId)| {
                            decision == &pending.decision
                        },
                    )
            });
            if !pending.is_empty() {
                let resolved = self
                    .resolve_npc_decisions(
                        pending,
                        &settled_materializations,
                        before.player.actor_id,
                        player_transcript.id,
                        &input,
                        &mut generated_attempts,
                        &mut orchestration,
                        &mut tool_activity,
                        &mut notices,
                        started,
                        on_phase,
                    )
                    .await?;
                settled_materializations.extend(resolved.iter().filter_map(|result| {
                    (result.status == NpcMaterializationStatus::Materialized)
                        .then_some(result.actor_id)
                        .flatten()
                        .map(|actor_id| (result.decision.clone(), actor_id))
                }));
                materialization_results.extend(resolved);
            }
            if requires_replanning {
                pending_turns.clear();
                continue;
            }

            let plan_revision = self.service.revision().await;
            for request in &mut pending_turns {
                request.based_on_revision = plan_revision;
            }
            let plan = NarratorPlan {
                based_on_revision: plan_revision,
                npc_turns: pending_turns,
            };
            plan.validate()?;

            if plan.npc_turns.is_empty() {
                let narration = loreloom_agent::NarrationText::new(narrator_text)?;
                let supporting_events = self
                    .service
                    .events()
                    .await
                    .into_iter()
                    .filter(|event| event.revision > before.revision)
                    .map(|event| event.id)
                    .collect::<Vec<_>>();
                on_phase(RuntimePhase::UpdatingWorld);
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
                        notices,
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
                if !self.npcs.contains_key(&request.actor_id)
                    && let Ok(definition) = self.service.agent_definition(request.actor_id).await
                {
                    self.npcs.insert(
                        request.actor_id,
                        NpcRegistration {
                            definition,
                            bridge: Arc::clone(&self.default_npc_bridge),
                        },
                    );
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
                let (mut character, mut scene, mut dialogue) = match self
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
                let context_truncated = project_npc_context(
                    &mut character,
                    &mut scene,
                    &mut dialogue,
                    self.config.context_projection,
                );
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
                    context_truncated,
                )?;
                let npc_request = agent.request(self.runner.definitions())?;
                on_phase(RuntimePhase::NpcThinking);
                let turn = self
                    .runner
                    .run_turn(TurnInvocation {
                        model_invocation: ModelInvocationKind::Npc,
                        bridge: registration.bridge.as_ref(),
                        request: npc_request,
                        tool_context: AgentToolContext {
                            actor_id: request.actor_id,
                            revision: observed_revision,
                            session_id: self.session_id,
                            capabilities: registration.definition.allowed_tools.clone(),
                        },
                        budget: self.config.turn_budget,
                        max_context_tokens: self.config.context_projection.max_context_tokens,
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
                let (status, response, failure) = npc_output(&turn);
                if let NpcTurnStatus::BudgetExhausted(reason) = status {
                    budget_failure = Some(reason);
                }
                if let Some(diagnostic) = &failure {
                    notices.push(model_failure_notice(diagnostic)?);
                }
                npc_results.push(NpcTurnResult {
                    request_id: request.request_id,
                    actor_id: request.actor_id,
                    observed_revision: Some(observed_revision),
                    final_revision: self.service.revision().await,
                    status,
                    response,
                    failure,
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
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn resolve_npc_decisions<F>(
        &mut self,
        pending: Vec<PendingNpcDecision>,
        settled_materializations: &[(NarratorNpcDecision, ActorId)],
        acting_actor: ActorId,
        source_transcript: TranscriptItemId,
        player_input: &LongText,
        generated_attempts: &mut u32,
        orchestration: &mut OrchestrationState,
        tool_activity: &mut Vec<ToolActivity>,
        notices: &mut Vec<UiNotice>,
        started: Instant,
        on_phase: &mut F,
    ) -> Result<Vec<NpcMaterializationResult>, RuntimeError>
    where
        F: FnMut(RuntimePhase),
    {
        let mut outcomes = Vec::with_capacity(pending.len());
        for pending in pending {
            if self.cancellation.is_cancelled() {
                return Err(RuntimeError::Cancelled);
            }
            if pending.revision != self.service.revision().await {
                outcomes.push(NpcMaterializationResult::rejected(
                    pending,
                    self.service.revision().await,
                    "stale_revision",
                ));
                continue;
            }
            let decision = pending.decision.clone();
            if let Some((_, actor_id)) = settled_materializations
                .iter()
                .find(|(settled, _)| settled == &decision)
                .map(|(settled, actor_id)| (settled, *actor_id))
                .or_else(|| {
                    outcomes
                        .iter()
                        .find_map(|outcome: &NpcMaterializationResult| {
                            (outcome.decision == decision
                                && matches!(
                                    outcome.status,
                                    NpcMaterializationStatus::Materialized
                                        | NpcMaterializationStatus::Existing
                                ))
                            .then_some(outcome.actor_id)
                            .flatten()
                            .map(|actor_id| (&outcome.decision, actor_id))
                        })
                })
            {
                outcomes.push(NpcMaterializationResult::settled(
                    pending,
                    NpcMaterializationStatus::Existing,
                    Some(actor_id),
                    self.service.revision().await,
                ));
                continue;
            }
            match &decision.target {
                NpcTarget::Mentioned { .. } => {
                    outcomes.push(NpcMaterializationResult::settled(
                        pending,
                        NpcMaterializationStatus::Mentioned,
                        None,
                        self.service.revision().await,
                    ));
                }
                NpcTarget::Existing { actor_id } => {
                    let valid = self.service.has_character(*actor_id).await
                        && match &decision.controller {
                            NpcControllerKind::Agent(profile_id)
                                if decision.action == NpcNarrativeAction::RequestNpcTurn =>
                            {
                                self.service.agent_profile(*actor_id).await.as_ref()
                                    == Some(profile_id)
                            }
                            _ => true,
                        };
                    if valid {
                        outcomes.push(NpcMaterializationResult::settled(
                            pending,
                            NpcMaterializationStatus::Existing,
                            Some(*actor_id),
                            self.service.revision().await,
                        ));
                    } else {
                        outcomes.push(NpcMaterializationResult::rejected(
                            pending,
                            self.service.revision().await,
                            "existing_target_unavailable",
                        ));
                    }
                }
                NpcTarget::Preset {
                    character_id,
                    place_id,
                } => {
                    let mut observation = self
                        .service
                        .observation(self.session_id, player_input.clone())
                        .await?;
                    project_observation(&mut observation, self.config.context_projection);
                    let scene_id = observation.scene.scene_id;
                    if let Some(reason) = self.materialization_limit(scene_id, false).await? {
                        outcomes.push(NpcMaterializationResult::rejected(
                            pending,
                            self.service.revision().await,
                            reason,
                        ));
                        continue;
                    }
                    let (controller, required_profile) = controller(&decision.controller);
                    let lifetime = lifetime(decision.lifetime, scene_id)?;
                    let materialization = CharacterMaterializationRequest {
                        acting_actor,
                        scene_id,
                        place_id: *place_id,
                        controller,
                        lifetime,
                        required_agent_profile: required_profile.cloned(),
                    };
                    on_phase(RuntimePhase::UpdatingWorld);
                    match self
                        .service
                        .spawn_preset_character(character_id, &materialization)
                        .await
                    {
                        Ok(actor_id) => {
                            self.register_materialized_agent(actor_id).await?;
                            outcomes.push(NpcMaterializationResult::settled(
                                pending,
                                NpcMaterializationStatus::Materialized,
                                Some(actor_id),
                                self.service.revision().await,
                            ));
                        }
                        Err(error) => outcomes.push(NpcMaterializationResult::rejected(
                            pending,
                            self.service.revision().await,
                            error.code(),
                        )),
                    }
                }
                NpcTarget::Generated {
                    generation_policy_id,
                    place_id,
                    request,
                } => {
                    if *generated_attempts
                        >= self.config.npc_resources.max_generated_per_orchestration
                    {
                        outcomes.push(NpcMaterializationResult::rejected(
                            pending,
                            self.service.revision().await,
                            "generation_limit",
                        ));
                        continue;
                    }
                    let Some(policy) = self
                        .config
                        .generation_policies
                        .get(generation_policy_id)
                        .cloned()
                    else {
                        outcomes.push(NpcMaterializationResult::rejected(
                            pending,
                            self.service.revision().await,
                            "generation_policy_unavailable",
                        ));
                        continue;
                    };
                    let observation = self
                        .service
                        .observation(self.session_id, player_input.clone())
                        .await?;
                    let scene_id = observation.scene.scene_id;
                    if request.scene_id != scene_id
                        || !request
                            .desired_traits
                            .is_subset(&policy.constraints.allowed_definitions)
                    {
                        outcomes.push(NpcMaterializationResult::rejected(
                            pending,
                            self.service.revision().await,
                            "generation_request_not_authorized",
                        ));
                        continue;
                    }
                    let persistent = decision.lifetime == NpcLifetime::Persistent;
                    if let Some(reason) = self.materialization_limit(scene_id, persistent).await? {
                        outcomes.push(NpcMaterializationResult::rejected(
                            pending,
                            self.service.revision().await,
                            reason,
                        ));
                        continue;
                    }
                    *generated_attempts += 1;
                    orchestration.start_turn(self.config.orchestration_budget, started)?;
                    let definitions = match self.service.generation_definitions(&policy).await {
                        Ok(definitions) => definitions,
                        Err(error) => {
                            outcomes.push(NpcMaterializationResult::rejected(
                                pending,
                                self.service.revision().await,
                                error.code(),
                            ));
                            continue;
                        }
                    };
                    let _ = self.executor.take_pending_npc_drafts().await;
                    let generation_request = narrator_request(
                        "npc_generation",
                        json!({
                            "observation": observation,
                            "request": request,
                            "generation_policy": policy,
                            "allowed_definitions": definitions,
                            "required_agent_profile": match &decision.controller {
                                NpcControllerKind::Agent(profile_id) => Some(profile_id),
                                _ => None,
                            },
                            "instructions": "Create one NPC and submit it through submit_npc_draft. Do not return the draft as JSON text. After the tool result, finish with a short natural-language acknowledgement."
                        }),
                        self.runner
                            .definitions()
                            .into_iter()
                            .filter(|definition| definition.name == "submit_npc_draft")
                            .collect(),
                    )?;
                    on_phase(RuntimePhase::NarratorThinking);
                    let generation = self
                        .runner
                        .run_turn(TurnInvocation {
                            model_invocation: ModelInvocationKind::NpcGeneration,
                            bridge: self.narrator.as_ref(),
                            request: generation_request,
                            tool_context: AgentToolContext {
                                actor_id: acting_actor,
                                revision: self.service.revision().await,
                                session_id: self.session_id,
                                capabilities: BTreeSet::from([
                                    NARRATOR_SUBMIT_NPC_DRAFT_CAPABILITY.to_owned(),
                                ]),
                            },
                            budget: self.config.turn_budget,
                            max_context_tokens: self.config.context_projection.max_context_tokens,
                            cancellation: &self.cancellation,
                        })
                        .await;
                    append_tool_activity(tool_activity, &generation.tool_calls);
                    orchestration.merge(
                        &generation.usage,
                        self.config.orchestration_budget,
                        started,
                    )?;
                    let draft = match require_text(&generation) {
                        Ok(_) => {
                            let mut drafts = self.executor.take_pending_npc_drafts().await;
                            if drafts.len() == 1 {
                                drafts.pop().expect("one draft was checked above")
                            } else {
                                outcomes.push(NpcMaterializationResult::rejected(
                                    pending,
                                    self.service.revision().await,
                                    "invalid_npc_draft",
                                ));
                                continue;
                            }
                        }
                        Err(RuntimeError::Cancelled) => return Err(RuntimeError::Cancelled),
                        Err(RuntimeError::Budget(reason)) => {
                            return Err(RuntimeError::Budget(reason));
                        }
                        Err(RuntimeError::BridgeUnavailable(diagnostic)) => {
                            notices.push(model_failure_notice(&diagnostic)?);
                            outcomes.push(NpcMaterializationResult::model_failure(
                                pending,
                                self.service.revision().await,
                                diagnostic,
                            ));
                            continue;
                        }
                        Err(error) => {
                            outcomes.push(NpcMaterializationResult::rejected(
                                pending,
                                self.service.revision().await,
                                error.code(),
                            ));
                            continue;
                        }
                    };
                    let required_profile = match &decision.controller {
                        NpcControllerKind::Agent(profile_id) => Some(profile_id),
                        _ => None,
                    };
                    if draft.agent_profile.as_ref() != required_profile {
                        outcomes.push(NpcMaterializationResult::rejected(
                            pending,
                            self.service.revision().await,
                            "generated_agent_profile_mismatch",
                        ));
                        continue;
                    }
                    let (controller, _) = controller(&decision.controller);
                    let lifetime = lifetime(decision.lifetime, scene_id)?;
                    let materialization = CharacterMaterializationRequest {
                        acting_actor,
                        scene_id,
                        place_id: *place_id,
                        controller,
                        lifetime,
                        required_agent_profile: required_profile.cloned(),
                    };
                    let origin = GeneratedOrigin {
                        generation_id: GenerationId::new(),
                        generator_version: ShortText::new("npc_generation.v1")?,
                        source: GenerationSource::PlayerInput {
                            transcript_id: source_transcript,
                        },
                    };
                    on_phase(RuntimePhase::UpdatingWorld);
                    match self
                        .service
                        .spawn_generated_character(&draft, &policy, origin, &materialization)
                        .await
                    {
                        Ok(actor_id) => {
                            self.register_materialized_agent(actor_id).await?;
                            outcomes.push(NpcMaterializationResult::settled(
                                pending,
                                NpcMaterializationStatus::Materialized,
                                Some(actor_id),
                                self.service.revision().await,
                            ));
                        }
                        Err(error) => outcomes.push(NpcMaterializationResult::rejected(
                            pending,
                            self.service.revision().await,
                            error.code(),
                        )),
                    }
                }
            }
        }
        Ok(outcomes)
    }

    async fn materialization_limit(
        &self,
        scene_id: loreloom_core::ObjectId,
        persistent_generated: bool,
    ) -> Result<Option<&'static str>, RuntimeError> {
        let counts = self.service.materialization_counts(scene_id).await?;
        let scene_limit = usize::try_from(self.config.npc_resources.max_materialized_per_scene)
            .unwrap_or(usize::MAX);
        let persistent_limit = usize::try_from(self.config.npc_resources.max_persistent_generated)
            .unwrap_or(usize::MAX);
        if counts.scene_characters >= scene_limit {
            Ok(Some("scene_materialization_limit"))
        } else if persistent_generated && counts.persistent_generated >= persistent_limit {
            Ok(Some("persistent_generation_limit"))
        } else {
            Ok(None)
        }
    }

    async fn register_materialized_agent(&mut self, actor_id: ActorId) -> Result<(), RuntimeError> {
        if let Some(profile_id) = self.service.agent_profile(actor_id).await {
            let definition = self.service.agent_definition(actor_id).await?;
            if definition.profile_id != profile_id {
                return Err(RuntimeError::Unavailable);
            }
            self.npcs.insert(
                actor_id,
                NpcRegistration {
                    definition,
                    bridge: Arc::clone(&self.default_npc_bridge),
                },
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum NpcMaterializationStatus {
    Mentioned,
    Existing,
    Materialized,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct NpcMaterializationResult {
    call_id: String,
    decision: NarratorNpcDecision,
    status: NpcMaterializationStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    actor_id: Option<ActorId>,
    revision: Revision,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure: Option<ModelFailureDiagnostic>,
}

impl NpcMaterializationResult {
    fn settled(
        pending: PendingNpcDecision,
        status: NpcMaterializationStatus,
        actor_id: Option<ActorId>,
        revision: Revision,
    ) -> Self {
        Self {
            call_id: pending.call_id,
            decision: pending.decision,
            status,
            actor_id,
            revision,
            reason: None,
            failure: None,
        }
    }

    fn rejected(pending: PendingNpcDecision, revision: Revision, reason: &str) -> Self {
        Self {
            call_id: pending.call_id,
            decision: pending.decision,
            status: NpcMaterializationStatus::Rejected,
            actor_id: None,
            revision,
            reason: Some(reason.to_owned()),
            failure: None,
        }
    }

    fn model_failure(
        pending: PendingNpcDecision,
        revision: Revision,
        failure: ModelFailureDiagnostic,
    ) -> Self {
        Self {
            call_id: pending.call_id,
            decision: pending.decision,
            status: NpcMaterializationStatus::Rejected,
            actor_id: None,
            revision,
            reason: Some("bridge_unavailable".to_owned()),
            failure: Some(failure),
        }
    }
}

fn controller(kind: &NpcControllerKind) -> (CharacterController, Option<&ContentDefinitionId>) {
    match kind {
        NpcControllerKind::NarratorProxy => (CharacterController::NarratorProxy, None),
        NpcControllerKind::Rules => (CharacterController::Rules, None),
        NpcControllerKind::Agent(profile_id) => (CharacterController::Agent, Some(profile_id)),
    }
}

fn lifetime(
    kind: NpcLifetime,
    scene_id: loreloom_core::ObjectId,
) -> Result<CharacterLifetime, RuntimeError> {
    match kind {
        NpcLifetime::Beat => Err(RuntimeError::InvalidInput),
        NpcLifetime::Scene => Ok(CharacterLifetime::Scene { scene_id }),
        NpcLifetime::Persistent => Ok(CharacterLifetime::Persistent),
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
                    "Player input goes only to the narrator. Use native tools for every structured decision or world change. Return natural-language prose in the response body; never return JSON or a structured control envelope. NPC claims are not committed facts.",
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
        TurnStatus::Failed(_) => match outcome.failure.clone() {
            Some(diagnostic) => Err(RuntimeError::BridgeUnavailable(diagnostic)),
            None => Err(RuntimeError::ModelProtocol {
                stage: "missing_failure_diagnostic",
            }),
        },
    }
}

fn npc_output(
    outcome: &TurnOutcome,
) -> (
    NpcTurnStatus,
    Option<LongText>,
    Option<ModelFailureDiagnostic>,
) {
    match outcome.status {
        TurnStatus::Completed => match outcome.final_text.clone().map(LongText::new) {
            Some(Ok(response)) => (NpcTurnStatus::Completed, Some(response), None),
            _ => (NpcTurnStatus::Failed, None, None),
        },
        TurnStatus::Cancelled => (NpcTurnStatus::Cancelled, None, None),
        TurnStatus::BudgetExhausted(reason) => (NpcTurnStatus::BudgetExhausted(reason), None, None),
        TurnStatus::Failed(_) => (NpcTurnStatus::Failed, None, outcome.failure.clone()),
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
        response: None,
        failure: None,
        tool_call_ids: Vec::new(),
        world_events: Vec::new(),
    }
}

fn model_failure_notice(diagnostic: &ModelFailureDiagnostic) -> Result<UiNotice, RuntimeError> {
    Ok(UiNotice {
        kind: NoticeKind::Warning,
        message: ShortText::new(format!(
            "Model request failed · {}",
            diagnostic.user_summary()
        ))?,
    })
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
