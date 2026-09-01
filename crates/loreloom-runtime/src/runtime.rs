use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Instant,
};

use armillae_core::{CompletionRequest, ContentPart, Message, Role};
use armillae_llm::LlmBridge;
use loreloom_agent::{
    AgentDefinition, AgentRunner, AgentToolContext, BudgetReason, CancellationToken,
    ModelFailureDiagnostic, ModelInvocationKind, NarratorDefinition, NarratorNpcDecision,
    NarratorPlan, NpcAgent, NpcAssignment, NpcControllerKind, NpcLifetime, NpcNarrativeAction,
    NpcTarget, NpcTurnRequest, NpcTurnResult, NpcTurnStatus, ResourceUsage, ToolCallOutcome,
    ToolCallProgress, TurnInvocation, TurnOutcome, TurnStatus,
};
use loreloom_core::{
    ActorId, CharacterController, CharacterLifetime, ContentDefinitionId, GeneratedOrigin,
    GenerationId, GenerationSource, LongText, ModPackageStatus, ModPackageView, NoticeKind,
    PackageCatalogView, Revision, RuntimePhase, RuntimeProgressEvent, SessionId, ShortText,
    ToolActivity, ToolActivityState, TranscriptItemId, TranscriptSpeaker, UiNotice, UiSnapshot,
    WorldCommandKind,
};
use serde::Serialize;
use serde_json::json;

use crate::{
    NARRATOR_CREATE_NPC_CAPABILITY, NARRATOR_CREATE_PLACE_CAPABILITY,
    NARRATOR_CREATE_SCENE_CAPABILITY, NARRATOR_REQUEST_NPC_TURN_CAPABILITY,
    NARRATOR_SUBMIT_NPC_DRAFT_CAPABILITY, NARRATOR_TRANSITION_SCENE_CAPABILITY,
    OrchestrationBudget, RuntimeConfig, RuntimeError, RuntimeToolExecutor, WorldService,
    context::{project_npc_context, project_observation},
    world_service::{CharacterMaterializationRequest, PendingNpcDecision, PendingTopologyKind},
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
    narrator_definition: NarratorDefinition,
    default_npc_bridge: Arc<dyn LlmBridge>,
    npcs: BTreeMap<ActorId, NpcRegistration>,
    session_id: SessionId,
    config: RuntimeConfig,
    cancellation: CancellationToken,
    installed_mods: Vec<ModPackageView>,
    unavailable_installed_mods: u32,
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
        narrator_definition: NarratorDefinition,
        session_id: SessionId,
        config: RuntimeConfig,
    ) -> Self {
        let executor = Arc::new(RuntimeToolExecutor::with_generation_policy(
            Arc::clone(&service),
            config.generation_policy.clone(),
        ));
        Self {
            service,
            executor: Arc::clone(&executor),
            runner: AgentRunner::new(executor),
            narrator: Arc::clone(&narrator),
            narrator_definition,
            default_npc_bridge: narrator,
            npcs: BTreeMap::new(),
            session_id,
            config,
            cancellation: CancellationToken::new(),
            installed_mods: Vec::new(),
            unavailable_installed_mods: 0,
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

    pub fn set_installed_mod_catalog(
        &mut self,
        mut installed_mods: Vec<ModPackageView>,
        unavailable: u32,
    ) {
        installed_mods.retain(|package| package.status == ModPackageStatus::Installed);
        installed_mods.sort_by(|left, right| {
            left.mod_id
                .cmp(&right.mod_id)
                .then_with(|| left.version.cmp(&right.version))
        });
        installed_mods
            .dedup_by(|left, right| left.mod_id == right.mod_id && left.version == right.version);
        self.installed_mods = installed_mods;
        self.unavailable_installed_mods = unavailable;
    }

    #[must_use]
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation.clone()
    }

    pub async fn initial_snapshot(&self) -> Result<UiSnapshot, RuntimeError> {
        let mut snapshot = self
            .service
            .snapshot(
                self.session_id,
                RuntimePhase::Idle,
                Vec::new(),
                Vec::new(),
                Vec::new(),
            )
            .await?;
        self.attach_installed_mods(&mut snapshot);
        Ok(snapshot)
    }

    pub fn handle_player_input(
        &mut self,
        input: impl Into<String>,
    ) -> impl Future<Output = Result<PlayerTurnOutcome, RuntimeError>> + '_ {
        self.handle_player_input_with_progress(input, |_| {})
    }

    pub fn handle_player_input_with_phase<'a, F>(
        &'a mut self,
        input: impl Into<String>,
        mut on_phase: F,
    ) -> impl Future<Output = Result<PlayerTurnOutcome, RuntimeError>> + 'a
    where
        F: FnMut(RuntimePhase) + 'a,
    {
        self.handle_player_input_with_progress(input, move |event| {
            if let RuntimeProgressEvent::PhaseChanged(phase) = event {
                on_phase(phase);
            }
        })
    }

    pub fn handle_player_input_with_progress<'a, F>(
        &'a mut self,
        input: impl Into<String>,
        mut on_progress: F,
    ) -> impl Future<Output = Result<PlayerTurnOutcome, RuntimeError>> + 'a
    where
        F: FnMut(RuntimeProgressEvent) + 'a,
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
            self.handle_valid_player_input(input, &mut on_progress)
                .await
        }
    }

    async fn handle_valid_player_input<F>(
        &mut self,
        input: LongText,
        on_progress: &mut F,
    ) -> Result<PlayerTurnOutcome, RuntimeError>
    where
        F: FnMut(RuntimeProgressEvent),
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
        let _ = self.executor.take_pending_topology().await;

        let before = self
            .service
            .observation(self.session_id, input.clone())
            .await?;
        publish_phase(on_progress, RuntimePhase::PersistingInput);
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
        let mut world_topology_results = Vec::new();
        let mut settled_materializations = Vec::new();
        let mut generated_attempts = 0_u32;
        loop {
            publish_phase(on_progress, RuntimePhase::NarratorThinking);
            orchestration.start_round(self.config.orchestration_budget, started)?;
            orchestration.start_turn(self.config.orchestration_budget, started)?;
            let mut observation = self
                .service
                .observation(self.session_id, input.clone())
                .await?;
            project_observation(&mut observation, self.config.context_projection);
            let tools = narrator_tools(
                self.runner.definitions(),
                &observation,
                &self.config.narrator_capabilities,
            );
            let narrator_request = narrator_request(
                "narrator_turn",
                json!({
                    "observation": observation,
                    "materialization_results": materialization_results,
                    "world_topology_results": world_topology_results,
                    "npc_results": npc_results,
                    "committed_events": self.service.events().await
                }),
                tools,
                &self.narrator_definition,
            )?;
            let narrator_turn = self
                .runner
                .run_turn_with_progress(
                    TurnInvocation {
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
                    },
                    |progress| {
                        publish_tool_progress(&mut tool_activity, progress, on_progress);
                    },
                )
                .await;
            orchestration.merge(
                &narrator_turn.usage,
                self.config.orchestration_budget,
                started,
            )?;
            let pending = self.executor.take_pending_npc_decisions().await;
            let mut pending_turns = self.executor.take_pending_npc_turns().await;
            let pending_topology = self.executor.take_pending_topology().await;
            let narrator_text = require_text(&narrator_turn)?;
            publish_phase(on_progress, RuntimePhase::ResolvingOrchestration);
            if let Some(pending_topology) = pending_topology {
                let context = AgentToolContext {
                    actor_id: before.player.actor_id,
                    revision: pending_topology.revision,
                    session_id: self.session_id,
                    capabilities: self.config.narrator_capabilities.clone(),
                };
                let origin = GeneratedOrigin {
                    generation_id: GenerationId::new(),
                    generator_version: ShortText::new("world_topology.v1")?,
                    source: GenerationSource::PlayerInput {
                        transcript_id: player_transcript.id,
                    },
                };
                let (request, command) = match pending_topology.kind {
                    PendingTopologyKind::Transition { target } => (
                        json!({ "type": "transition", "target": target }),
                        WorldCommandKind::TransitionScene { target },
                    ),
                    PendingTopologyKind::CreateScene {
                        display_name,
                        framing,
                        entry_place_name,
                        entry_place_description,
                    } => (
                        json!({ "type": "create_scene", "display_name": display_name }),
                        WorldCommandKind::CreateScene {
                            display_name,
                            framing,
                            entry_place_name,
                            entry_place_description,
                            origin,
                        },
                    ),
                    PendingTopologyKind::CreatePlace {
                        display_name,
                        description,
                    } => (
                        json!({ "type": "create_place", "display_name": display_name }),
                        WorldCommandKind::CreatePlace {
                            display_name,
                            description,
                            origin,
                        },
                    ),
                };
                publish_phase(on_progress, RuntimePhase::UpdatingWorld);
                let result = self.service.execute(&context, command).await;
                match result {
                    Ok(committed) => {
                        let events = self
                            .service
                            .events()
                            .await
                            .into_iter()
                            .filter(|event| committed.event_ids.contains(&event.id))
                            .collect::<Vec<_>>();
                        world_topology_results.push(json!({
                            "call_id": pending_topology.call_id,
                            "request": request,
                            "status": "committed",
                            "revision": committed.revision,
                            "events": events
                        }));
                    }
                    Err(error @ (RuntimeError::World(_) | RuntimeError::Content(_))) => {
                        world_topology_results.push(json!({
                            "call_id": pending_topology.call_id,
                            "request": request,
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
                        on_progress,
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
                publish_phase(on_progress, RuntimePhase::UpdatingWorld);
                self.service
                    .append_transcript(
                        before.player.actor_id,
                        self.session_id,
                        TranscriptSpeaker::Narrator,
                        narration.clone(),
                        supporting_events.clone(),
                    )
                    .await?;
                let mut snapshot = self
                    .service
                    .snapshot(
                        self.session_id,
                        RuntimePhase::Completed,
                        tool_activity,
                        notices,
                        supporting_events,
                    )
                    .await?;
                self.attach_installed_mods(&mut snapshot);
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
                let npc_request = agent.request(
                    self.runner.definitions(),
                    &self.narrator_definition.npc_prompts,
                )?;
                publish_phase(on_progress, RuntimePhase::NpcThinking);
                let turn = self
                    .runner
                    .run_turn_with_progress(
                        TurnInvocation {
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
                        },
                        |progress| {
                            publish_tool_progress(&mut tool_activity, progress, on_progress);
                        },
                    )
                    .await;
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
        on_progress: &mut F,
    ) -> Result<Vec<NpcMaterializationResult>, RuntimeError>
    where
        F: FnMut(RuntimeProgressEvent),
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
                    publish_phase(on_progress, RuntimePhase::UpdatingWorld);
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
                        .generation_policy
                        .as_ref()
                        .filter(|policy| &policy.id == generation_policy_id)
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
                            }
                        }),
                        self.runner
                            .definitions()
                            .into_iter()
                            .filter(|definition| definition.name == "submit_npc_draft")
                            .collect(),
                        &self.narrator_definition,
                    )?;
                    publish_phase(on_progress, RuntimePhase::NarratorThinking);
                    let generation = self
                        .runner
                        .run_turn_with_progress(
                            TurnInvocation {
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
                                max_context_tokens: self
                                    .config
                                    .context_projection
                                    .max_context_tokens,
                                cancellation: &self.cancellation,
                            },
                            |progress| {
                                publish_tool_progress(tool_activity, progress, on_progress);
                            },
                        )
                        .await;
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
                    publish_phase(on_progress, RuntimePhase::UpdatingWorld);
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

    fn attach_installed_mods(&self, snapshot: &mut UiSnapshot) {
        merge_installed_mods(
            &mut snapshot.packages,
            &self.installed_mods,
            self.unavailable_installed_mods,
        );
    }
}

fn merge_installed_mods(
    catalog: &mut PackageCatalogView,
    installed_mods: &[ModPackageView],
    unavailable: u32,
) {
    let enabled = catalog
        .mods
        .iter()
        .filter(|package| package.status == ModPackageStatus::Enabled)
        .map(|package| (package.mod_id.clone(), package.version.clone()))
        .collect::<BTreeSet<_>>();
    catalog.mods.extend(
        installed_mods
            .iter()
            .filter(|package| {
                package.status == ModPackageStatus::Installed
                    && !enabled.contains(&(package.mod_id.clone(), package.version.clone()))
            })
            .cloned(),
    );
    catalog.mods.sort_by(|left, right| {
        left.status
            .cmp(&right.status)
            .then_with(|| left.mod_id.cmp(&right.mod_id))
            .then_with(|| left.version.cmp(&right.version))
    });
    catalog.mods.dedup_by(|left, right| {
        left.mod_id == right.mod_id && left.version == right.version && left.status == right.status
    });
    catalog.unavailable_installed = unavailable;
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

fn narrator_tools(
    definitions: Vec<armillae_core::ToolDefinition>,
    observation: &loreloom_core::SceneObservation,
    capabilities: &BTreeSet<String>,
) -> Vec<armillae_core::ToolDefinition> {
    let available_actor_ids = observation
        .scene
        .visible_actors
        .iter()
        .filter(|actor| actor.npc_turn_available)
        .map(|actor| json!(actor.actor_id))
        .collect::<Vec<_>>();
    definitions
        .into_iter()
        .filter_map(|mut definition| {
            let authorized = match definition.name.as_str() {
                "submit_npc_draft" => false,
                "create_npc" => capabilities.contains(NARRATOR_CREATE_NPC_CAPABILITY),
                "create_scene" => capabilities.contains(NARRATOR_CREATE_SCENE_CAPABILITY),
                "create_place" => capabilities.contains(NARRATOR_CREATE_PLACE_CAPABILITY),
                "request_npc_turn" => {
                    capabilities.contains(NARRATOR_REQUEST_NPC_TURN_CAPABILITY)
                        && !available_actor_ids.is_empty()
                }
                "list_scene_transitions" | "transition_scene" => {
                    capabilities.contains(NARRATOR_TRANSITION_SCENE_CAPABILITY)
                }
                _ => true,
            };
            if !authorized {
                return None;
            }
            if definition.name == "request_npc_turn"
                && let Some(actor_id) = definition.input_schema.pointer_mut("/properties/actor_id")
            {
                actor_id["enum"] = serde_json::Value::Array(available_actor_ids.clone());
            }
            Some(definition)
        })
        .collect()
}

fn narrator_request(
    kind: &'static str,
    payload: serde_json::Value,
    tools: Vec<armillae_core::ToolDefinition>,
    narrator: &NarratorDefinition,
) -> Result<CompletionRequest, RuntimeError> {
    let payload = serde_json::to_string(&json!({ "kind": kind, "payload": payload }))
        .map_err(|error| RuntimeError::json("narrator_context", error))?;
    let mut messages = vec![Message::new(
        Role::System,
        vec![ContentPart::text(
            "Player input goes only to the narrator. Use native tools for every structured decision or world change. request_npc_turn accepts only an actor_id marked npc_turn_available in the current observation plus a natural-language assignment; scene and revision are supplied by the runtime. create_npc accepts only source, lifetime and mode; after creation the runtime replans with the committed actor before any NPC turn. Pure narrative mentions need no tool. When scene transition tools are offered, call list_scene_transitions first and copy one returned target exactly; never invent a scene ID, retry an unchanged rejection, or narrate arrival before a committed transition result. If no scene target matches, explain that the destination is unavailable in current world content. When submit_npc_draft is offered, submit exactly one NPC through that tool rather than returning draft data in text. Return only natural-language prose in the response body; never return JSON or a structured control envelope. Never expose tool failures or internal orchestration in player-facing prose. NPC claims are not committed facts; only committed events are world facts.",
        )],
    )];
    messages.extend(
        narrator
            .narrator_prompts
            .iter()
            .map(|prompt| Message::new(Role::System, vec![ContentPart::text(prompt.as_str())])),
    );
    messages.push(Message::user(payload));
    Ok(CompletionRequest {
        messages,
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

fn publish_phase<F>(on_progress: &mut F, phase: RuntimePhase)
where
    F: FnMut(RuntimeProgressEvent),
{
    on_progress(RuntimeProgressEvent::PhaseChanged(phase));
}

fn publish_tool_progress<F>(
    activity: &mut Vec<ToolActivity>,
    progress: ToolCallProgress,
    on_progress: &mut F,
) where
    F: FnMut(RuntimeProgressEvent),
{
    match progress {
        ToolCallProgress::Started { call_id, name } => activity.push(ToolActivity {
            call_id,
            name,
            state: ToolActivityState::Pending,
            code: None,
        }),
        ToolCallProgress::Finished(tool) => {
            let state = tool_activity_state(&tool);
            if let Some(pending) = activity.iter_mut().rev().find(|activity| {
                activity.call_id == tool.call_id
                    && activity.name == tool.name
                    && activity.state == ToolActivityState::Pending
            }) {
                pending.state = state;
                pending.code = tool.error_code;
            } else {
                activity.push(ToolActivity {
                    call_id: tool.call_id,
                    name: tool.name,
                    state,
                    code: tool.error_code,
                });
            }
        }
    }
    on_progress(RuntimeProgressEvent::ToolActivityChanged(activity.clone()));
}

fn tool_activity_state(tool: &ToolCallOutcome) -> ToolActivityState {
    if !tool.is_error {
        ToolActivityState::Succeeded
    } else if tool.error_code.as_deref() == Some("tool_execution_error") {
        ToolActivityState::Failed
    } else {
        ToolActivityState::Rejected
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use armillae_core::ContentPart;
    use loreloom_agent::NarratorDefinition;

    use super::*;

    fn text(message: &Message) -> &str {
        match &message.content[..] {
            [ContentPart::Text(value)] => value.text.as_str(),
            _ => panic!("single text message"),
        }
    }

    #[test]
    fn narrator_context_orders_engine_world_and_observation() {
        let definition = NarratorDefinition {
            narrator_prompts: vec![
                LongText::new("用克制的中文叙述这个世界。").expect("world narrator prompt"),
                LongText::new("雨声应当持续存在。").expect("Mod narrator prompt"),
            ],
            npc_prompts: vec![LongText::new("只根据已知事实行动。").expect("NPC prompt")],
        };
        let request = narrator_request(
            "narrator_turn",
            json!({ "observation": { "revision": 7 } }),
            Vec::new(),
            &definition,
        )
        .expect("narrator request");

        assert_eq!(request.messages.len(), 4);
        assert!(text(&request.messages[0]).contains("native tools"));
        assert_eq!(text(&request.messages[1]), "用克制的中文叙述这个世界。");
        assert_eq!(text(&request.messages[2]), "雨声应当持续存在。");
        assert!(text(&request.messages[3]).contains("\"observation\""));
    }

    #[test]
    fn tool_progress_settles_the_matching_pending_activity_in_place() {
        let mut activity = Vec::new();
        let mut observed = Vec::new();
        publish_tool_progress(
            &mut activity,
            ToolCallProgress::Started {
                call_id: "call-1".to_owned(),
                name: "narrator.create_scene".to_owned(),
            },
            &mut |event| observed.push(event),
        );
        publish_tool_progress(
            &mut activity,
            ToolCallProgress::Finished(ToolCallOutcome {
                call_id: "call-1".to_owned(),
                name: "narrator.create_scene".to_owned(),
                is_error: true,
                error_code: Some("stale_revision".to_owned()),
            }),
            &mut |event| observed.push(event),
        );

        assert_eq!(activity.len(), 1);
        assert_eq!(activity[0].state, ToolActivityState::Rejected);
        assert_eq!(activity[0].code.as_deref(), Some("stale_revision"));
        assert!(matches!(
            &observed[..],
            [
                RuntimeProgressEvent::ToolActivityChanged(pending),
                RuntimeProgressEvent::ToolActivityChanged(settled),
            ] if pending[0].state == ToolActivityState::Pending
                && settled[0].state == ToolActivityState::Rejected
        ));
    }

    #[test]
    fn installed_catalog_keeps_enabled_authority_and_stable_order() {
        let enabled = ModPackageView {
            mod_id: "games.loreloom.weather".parse().expect("enabled Mod ID"),
            version: "1.0.0".parse().expect("enabled version"),
            status: ModPackageStatus::Enabled,
            dependency_count: 1,
        };
        let mut catalog = PackageCatalogView {
            world: loreloom_core::WorldPackageView {
                world_id: "games.loreloom.world".parse().expect("world ID"),
                version: "1.0.0".parse().expect("world version"),
            },
            mods: vec![enabled.clone()],
            unavailable_installed: 0,
        };
        let installed = vec![
            ModPackageView {
                status: ModPackageStatus::Installed,
                ..enabled
            },
            ModPackageView {
                mod_id: "games.loreloom.characters"
                    .parse()
                    .expect("installed Mod ID"),
                version: "2.0.0".parse().expect("installed version"),
                status: ModPackageStatus::Installed,
                dependency_count: 0,
            },
        ];

        merge_installed_mods(&mut catalog, &installed, 2);

        assert_eq!(catalog.mods.len(), 2);
        assert_eq!(catalog.mods[0].status, ModPackageStatus::Enabled);
        assert_eq!(catalog.mods[1].status, ModPackageStatus::Installed);
        assert_eq!(catalog.mods[1].mod_id.as_str(), "games.loreloom.characters");
        assert_eq!(catalog.unavailable_installed, 2);
    }
}
