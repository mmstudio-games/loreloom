use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
};

use armillae_core::{ToolCall, ToolDefinition, ToolResult, ToolResultContent};
use armillae_tools::{BoxFuture, ToolContext, ToolExecutionError, ToolExecutor};
use loreloom_agent::{
    AgentDefinition, AgentToolContext, AssignmentText, CreateNpcRequest, NarrativeImportance,
    NarratorNpcDecision, NpcControllerKind, NpcCreationMode, NpcCreationSource,
    NpcGenerationRequest, NpcNarrativeAction, NpcTarget, NpcTurnRequest,
};
use loreloom_content::{
    CharacterCompileRequest, Definition, DefinitionRegistry, DraftCompileRequest, GenerationPolicy,
    NpcDraft, ParameterVisibility, PredicateDefinition, SceneSpawnPlan,
};
use loreloom_core::{
    ActionId, ActiveEventView, ActorId, AdjacentPlaceView, AttributeAdjustment, AttributeOperation,
    AttributeView, CharacterContext, CharacterController, CharacterLifetime, CharacterSpawnSpec,
    ConditionRecord, ConditionView, ContentDefinitionId, DIAGNOSED_CONDITION_PREDICATE_ID,
    DisplayName, DomainRecord, EventId, EventOptionView, FactSubject, FactValue, GeneratedOrigin,
    InventoryView, KnowledgeStatus, LongText, ModLock, ModPackageStatus, ModPackageView,
    NpcTurnRequestId, ObjectId, PackageCatalogView, ParameterSetView, ParameterValue,
    ParameterValueView, ResourceView, Revision, RuntimePhase, SAVE_FORMAT_V1, SaveId, SaveManifest,
    SceneContext, SceneObservation, SceneRecord, SceneTransitionTarget, SessionId, ShortText,
    SkillTargetRef, SkillView, SystemIdGenerator, ToolActivity, TranscriptWindow, UiNotice,
    UiSnapshot, VisibleActorView, WorldCommand, WorldCommandKind, WorldEvent, WorldEventKind,
    WorldLock, WorldPackageView,
};
use loreloom_store::{ActionResolution, CommitRequest, CommitResult, CommittedAction, SaveStore};
use loreloom_world::{GameWorld, WorldBootstrap, WorldConfig};
use serde_json::{Value as JsonValue, json};
use tokio::sync::Mutex;

use crate::{
    NARRATOR_CREATE_NPC_CAPABILITY, NARRATOR_CREATE_PLACE_CAPABILITY,
    NARRATOR_CREATE_SCENE_CAPABILITY, NARRATOR_REQUEST_NPC_TURN_CAPABILITY,
    NARRATOR_SUBMIT_NPC_DRAFT_CAPABILITY, NARRATOR_TRANSITION_SCENE_CAPABILITY, RuntimeError,
};

const CONTEXT_TRANSCRIPT_SOURCE_LIMIT: usize = 256;
const UI_TRANSCRIPT_LIMIT: usize = 64;
const CONTEXT_EVENT_LIMIT: usize = 64;
const TOOL_PAGE_DEFAULT: usize = 32;
const TOOL_PAGE_MAXIMUM: usize = 64;

struct RuntimeWorld {
    world: GameWorld,
    store: SaveStore,
    registry: DefinitionRegistry,
    config: WorldConfig,
    events: Vec<WorldEvent>,
    ids: SystemIdGenerator,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SceneTransitionOption {
    target: SceneTransitionTarget,
    display_name: DisplayName,
    framing: ShortText,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SceneTransitionProjection {
    current: SceneRecord,
    targets: Vec<SceneTransitionOption>,
}

pub struct WorldService {
    inner: Mutex<RuntimeWorld>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MaterializationCounts {
    pub scene_characters: usize,
    pub persistent_generated: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CharacterMaterializationRequest {
    pub acting_actor: ActorId,
    pub scene_id: ObjectId,
    pub place_id: ObjectId,
    pub controller: CharacterController,
    pub lifetime: CharacterLifetime,
    pub required_agent_profile: Option<ContentDefinitionId>,
}

impl std::fmt::Debug for WorldService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WorldService")
            .finish_non_exhaustive()
    }
}

impl WorldService {
    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        path: impl AsRef<Path>,
        save_id: SaveId,
        world_lock: WorldLock,
        mod_lock: ModLock,
        registry: DefinitionRegistry,
        plan: &SceneSpawnPlan,
        rng_seed: [u8; 32],
        config: WorldConfig,
    ) -> Result<(Arc<Self>, WorldBootstrap), RuntimeError> {
        let mut ids = SystemIdGenerator;
        let bootstrap = GameWorld::bootstrap(plan, rng_seed, &registry, config.clone(), &mut ids)?;
        let store = SaveStore::create(
            path,
            SaveManifest {
                format_version: SAVE_FORMAT_V1,
                save_id,
                world_id: bootstrap.world_id,
                world_lock: world_lock.clone(),
                mod_lock: mod_lock.clone(),
            },
            bootstrap.records.clone(),
        )
        .await?;
        let service = Self::open(store, registry, &world_lock, &mod_lock, config).await?;
        Ok((service, bootstrap))
    }

    pub async fn open(
        mut store: SaveStore,
        registry: DefinitionRegistry,
        candidate_world_lock: &WorldLock,
        candidate_mod_lock: &ModLock,
        config: WorldConfig,
    ) -> Result<Arc<Self>, RuntimeError> {
        let durable_world_lock = &store.manifest().world_lock;
        if durable_world_lock.world_id != candidate_world_lock.world_id
            || durable_world_lock.manifest_schema != candidate_world_lock.manifest_schema
            || durable_world_lock.content_schema != candidate_world_lock.content_schema
        {
            return Err(RuntimeError::ContentLockMismatch);
        }
        let loaded = store.load().await?;
        let world =
            GameWorld::from_records(loaded.revision, loaded.records, config.clone(), &registry)?;
        store
            .adopt_content_locks(candidate_world_lock.clone(), candidate_mod_lock.clone())
            .await?;
        Ok(Arc::new(Self {
            inner: Mutex::new(RuntimeWorld {
                world,
                store,
                registry,
                config,
                events: loaded.events,
                ids: SystemIdGenerator,
            }),
        }))
    }

    pub async fn revision(&self) -> Revision {
        self.inner.lock().await.world.revision()
    }

    pub async fn player_actor(&self) -> ActorId {
        self.inner.lock().await.world.world_state().player_actor
    }

    pub async fn agent_profile(
        &self,
        actor_id: ActorId,
    ) -> Option<loreloom_core::ContentDefinitionId> {
        self.inner
            .lock()
            .await
            .world
            .character(actor_id)
            .filter(|character| character.controller == CharacterController::Agent)
            .and_then(|character| character.agent_binding.as_ref())
            .filter(|binding| binding.enabled)
            .map(|binding| binding.profile_id.clone())
    }

    async fn npc_creation_location(
        &self,
        actor_id: ActorId,
        expected_revision: Revision,
    ) -> Result<(ObjectId, ObjectId), RuntimeError> {
        let inner = self.inner.lock().await;
        require_revision(&inner, expected_revision)?;
        if inner.world.world_state().player_actor != actor_id {
            return Err(RuntimeError::CapabilityDenied);
        }
        let character = inner
            .world
            .character(actor_id)
            .ok_or(RuntimeError::Unavailable)?;
        let records = inner.world.project_records()?;
        let place = records
            .iter()
            .find_map(|record| match record {
                DomainRecord::Place(place) if place.id == character.location => Some(place),
                _ => None,
            })
            .ok_or(RuntimeError::Unavailable)?;
        if place.scene_id != inner.world.world_state().active_scene {
            return Err(RuntimeError::Unavailable);
        }
        Ok((place.scene_id, place.id))
    }

    async fn preset_agent_profile(
        &self,
        character_id: &ContentDefinitionId,
    ) -> Result<ContentDefinitionId, RuntimeError> {
        let inner = self.inner.lock().await;
        let profile_id = inner
            .registry
            .get(character_id)
            .and_then(|entry| match &entry.definition {
                Definition::Character(character) => character.agent_profile.as_ref(),
                _ => None,
            })
            .ok_or(RuntimeError::Unavailable)?;
        match inner
            .registry
            .get(profile_id)
            .map(|entry| &entry.definition)
        {
            Some(Definition::AgentProfile(_)) => Ok(profile_id.clone()),
            _ => Err(RuntimeError::Unavailable),
        }
    }

    async fn npc_turn_scene(
        &self,
        player_actor: ActorId,
        npc_actor: ActorId,
        expected_revision: Revision,
    ) -> Result<ObjectId, RuntimeError> {
        let inner = self.inner.lock().await;
        require_revision(&inner, expected_revision)?;
        if inner.world.world_state().player_actor != player_actor {
            return Err(RuntimeError::CapabilityDenied);
        }
        let player = inner
            .world
            .character(player_actor)
            .ok_or(RuntimeError::Unavailable)?;
        let npc = inner
            .world
            .character(npc_actor)
            .filter(|npc| npc.location == player.location)
            .filter(|npc| npc.controller == CharacterController::Agent)
            .ok_or(RuntimeError::Unavailable)?;
        let binding = npc
            .agent_binding
            .as_ref()
            .filter(|binding| binding.enabled)
            .ok_or(RuntimeError::Unavailable)?;
        if !matches!(
            inner
                .registry
                .get(&binding.profile_id)
                .map(|entry| &entry.definition),
            Some(Definition::AgentProfile(_))
        ) {
            return Err(RuntimeError::Unavailable);
        }
        let records = inner.world.project_records()?;
        let place = records
            .iter()
            .find_map(|record| match record {
                DomainRecord::Place(place) if place.id == player.location => Some(place),
                _ => None,
            })
            .ok_or(RuntimeError::Unavailable)?;
        if place.scene_id != inner.world.world_state().active_scene {
            return Err(RuntimeError::Unavailable);
        }
        Ok(place.scene_id)
    }

    pub(crate) async fn has_character(&self, actor_id: ActorId) -> bool {
        self.inner.lock().await.world.character(actor_id).is_some()
    }

    pub async fn agent_definition(
        &self,
        actor_id: ActorId,
    ) -> Result<AgentDefinition, RuntimeError> {
        let inner = self.inner.lock().await;
        let binding = inner
            .world
            .character(actor_id)
            .filter(|character| character.controller == CharacterController::Agent)
            .and_then(|character| character.agent_binding.as_ref())
            .filter(|binding| binding.enabled)
            .ok_or(RuntimeError::Unavailable)?;
        let profile = inner
            .registry
            .get(&binding.profile_id)
            .and_then(|entry| match &entry.definition {
                Definition::AgentProfile(profile) => Some(profile),
                _ => None,
            })
            .ok_or(RuntimeError::Unavailable)?;
        Ok(AgentDefinition {
            profile_id: profile.id.clone(),
            system_style: LongText::new(profile.system_style.as_str())?,
            model_alias: profile.model_alias.clone(),
            allowed_tools: profile
                .tool_capabilities
                .iter()
                .map(|capability| capability.as_str().to_owned())
                .collect(),
        })
    }

    pub(crate) async fn generation_definitions(
        &self,
        policy: &GenerationPolicy,
    ) -> Result<Vec<Definition>, RuntimeError> {
        let inner = self.inner.lock().await;
        let ids = policy
            .constraints
            .allowed_definitions
            .iter()
            .chain(policy.allowed_agent_profiles.iter())
            .collect::<std::collections::BTreeSet<_>>();
        ids.into_iter()
            .map(|id| {
                inner
                    .registry
                    .get(id)
                    .map(|entry| entry.definition.clone())
                    .ok_or(RuntimeError::Unavailable)
            })
            .collect()
    }

    pub(crate) async fn materialization_counts(
        &self,
        scene_id: ObjectId,
    ) -> Result<MaterializationCounts, RuntimeError> {
        let inner = self.inner.lock().await;
        let player = inner.world.world_state().player_actor;
        let records = inner.world.project_records()?;
        let places = records
            .iter()
            .filter_map(|record| match record {
                DomainRecord::Place(place) => Some((place.id, place.scene_id)),
                _ => None,
            })
            .collect::<BTreeMap<_, _>>();
        let scene_characters = records
            .iter()
            .filter_map(|record| match record {
                DomainRecord::Character(character) => Some(character),
                _ => None,
            })
            .filter(|character| {
                character.id != player && places.get(&character.location) == Some(&scene_id)
            })
            .count();
        let persistent_generated = records
            .iter()
            .filter_map(|record| match record {
                DomainRecord::Character(character) => Some(character),
                _ => None,
            })
            .filter(|character| {
                character.lifetime == CharacterLifetime::Persistent
                    && matches!(
                        character.origin,
                        loreloom_core::EntityOrigin::Generated { .. }
                    )
            })
            .count();
        Ok(MaterializationCounts {
            scene_characters,
            persistent_generated,
        })
    }

    pub(crate) async fn spawn_preset_character(
        &self,
        character_id: &ContentDefinitionId,
        request: &CharacterMaterializationRequest,
    ) -> Result<ActorId, RuntimeError> {
        let mut inner = self.inner.lock().await;
        let spec = inner.registry.compile_character(
            character_id,
            CharacterCompileRequest {
                scene_id: request.scene_id,
                place_id: request.place_id,
                controller: request.controller,
                lifetime: request.lifetime,
            },
        )?;
        if spec
            .agent_binding
            .as_ref()
            .map(|binding| &binding.profile_id)
            != request.required_agent_profile.as_ref()
        {
            return Err(RuntimeError::Unavailable);
        }
        spawn_character(&mut inner, request.acting_actor, spec).await
    }

    pub(crate) async fn spawn_generated_character(
        &self,
        draft: &NpcDraft,
        policy: &GenerationPolicy,
        origin: GeneratedOrigin,
        request: &CharacterMaterializationRequest,
    ) -> Result<ActorId, RuntimeError> {
        let mut inner = self.inner.lock().await;
        let spec = inner.registry.compile_draft(
            draft,
            policy,
            DraftCompileRequest {
                origin,
                scene_id: request.scene_id,
                place_id: request.place_id,
                controller: request.controller,
                lifetime: request.lifetime,
            },
        )?;
        if spec
            .agent_binding
            .as_ref()
            .map(|binding| &binding.profile_id)
            != request.required_agent_profile.as_ref()
        {
            return Err(RuntimeError::Unavailable);
        }
        spawn_character(&mut inner, request.acting_actor, spec).await
    }

    pub async fn observation(
        &self,
        session_id: SessionId,
        player_input: LongText,
    ) -> Result<SceneObservation, RuntimeError> {
        let inner = self.inner.lock().await;
        let revision = inner.world.revision();
        let player_id = inner.world.world_state().player_actor;
        let records = inner.world.project_records()?;
        let player = character_context(&records, &inner.registry, player_id, revision)?;
        let scene = scene_context(
            &records,
            &inner.events,
            &inner.registry,
            player_id,
            revision,
        )?;
        let transcript = transcript_records(&records);
        let truncated = transcript.len() > CONTEXT_TRANSCRIPT_SOURCE_LIMIT;
        let recent_transcript = tail(transcript, CONTEXT_TRANSCRIPT_SOURCE_LIMIT);
        Ok(SceneObservation {
            revision,
            session_id,
            player,
            scene,
            recent_transcript,
            player_input,
            truncated,
        })
    }

    pub async fn npc_context(
        &self,
        actor_id: ActorId,
        scene_id: ObjectId,
    ) -> Result<
        (
            CharacterContext,
            SceneContext,
            Vec<loreloom_core::TranscriptItemRecord>,
        ),
        RuntimeError,
    > {
        let inner = self.inner.lock().await;
        let revision = inner.world.revision();
        let records = inner.world.project_records()?;
        let character = character_context(&records, &inner.registry, actor_id, revision)?;
        let scene = scene_context(&records, &inner.events, &inner.registry, actor_id, revision)?;
        if scene.scene_id != scene_id {
            return Err(RuntimeError::Unavailable);
        }
        let recent = tail(
            transcript_records(&records),
            CONTEXT_TRANSCRIPT_SOURCE_LIMIT,
        );
        Ok((character, scene, recent))
    }

    pub async fn events(&self) -> Vec<WorldEvent> {
        self.inner.lock().await.events.clone()
    }

    async fn inspect_character(
        &self,
        actor_id: ActorId,
        expected_revision: Revision,
    ) -> Result<CharacterContext, RuntimeError> {
        let inner = self.inner.lock().await;
        if inner.world.revision() != expected_revision {
            return Err(RuntimeError::Unavailable);
        }
        character_context(
            &inner.world.project_records()?,
            &inner.registry,
            actor_id,
            expected_revision,
        )
    }

    async fn list_inventory(
        &self,
        context: &AgentToolContext,
        after: Option<ObjectId>,
        limit: usize,
    ) -> Result<JsonValue, RuntimeError> {
        let inner = self.inner.lock().await;
        require_revision(&inner, context.revision)?;
        if inner.world.character(context.actor_id).is_none() {
            return Err(RuntimeError::Unavailable);
        }
        let records = inner.world.project_records()?;
        let mut items = records
            .iter()
            .filter_map(|record| match record {
                DomainRecord::Item(item)
                    if item.owned_by == Some(context.actor_id)
                        && after.is_none_or(|cursor| item.id > cursor) =>
                {
                    Some(item)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        items.sort_by_key(|item| item.id);
        let has_more = items.len() > limit;
        items.truncate(limit);
        let summaries = items
            .iter()
            .map(|item| {
                let Definition::Item(item_definition) =
                    definition(&inner.registry, &item.definition_id)?
                else {
                    return Err(RuntimeError::Unavailable);
                };
                Ok(json!({
                    "item_id": item.id,
                    "definition_id": item.definition_id,
                    "display_name": item_definition.display_name,
                    "custom_name": item.custom_name,
                    "quantity": item.stack.0.get(),
                    "durability": item.durability,
                    "contained_by": item.contained_by,
                    "equipped": item.equipped,
                    "is_container": item.container.is_some(),
                }))
            })
            .collect::<Result<Vec<_>, RuntimeError>>()?;
        let next_after = has_more.then(|| items.last().map(|item| item.id)).flatten();
        Ok(json!({
            "revision": context.revision,
            "items": summaries,
            "next_after": next_after,
        }))
    }

    async fn inspect_item(
        &self,
        context: &AgentToolContext,
        item_id: ObjectId,
    ) -> Result<JsonValue, RuntimeError> {
        let inner = self.inner.lock().await;
        require_revision(&inner, context.revision)?;
        let item = inner
            .world
            .item(item_id)
            .filter(|item| item.owned_by == Some(context.actor_id))
            .ok_or(RuntimeError::Unavailable)?;
        let Definition::Item(item_definition) = definition(&inner.registry, &item.definition_id)?
        else {
            return Err(RuntimeError::Unavailable);
        };
        Ok(json!({
            "revision": context.revision,
            "item": item,
            "definition": item_definition,
        }))
    }

    async fn list_available_skills(
        &self,
        context: &AgentToolContext,
        after: Option<ObjectId>,
        limit: usize,
    ) -> Result<JsonValue, RuntimeError> {
        let inner = self.inner.lock().await;
        require_revision(&inner, context.revision)?;
        let character = inner
            .world
            .character(context.actor_id)
            .ok_or(RuntimeError::Unavailable)?;
        let clock = inner.world.world_state().clock;
        let records = inner.world.project_records()?;
        let mut skills = records
            .iter()
            .filter_map(|record| match record {
                DomainRecord::SkillGrant(grant)
                    if grant.owner_id == context.actor_id
                        && after.is_none_or(|cursor| grant.id > cursor) =>
                {
                    Some(grant)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        skills.sort_by_key(|grant| grant.id);
        let has_more = skills.len() > limit;
        skills.truncate(limit);
        let summaries = skills
            .iter()
            .map(|grant| {
                let Definition::Skill(skill) = definition(&inner.registry, &grant.skill_id)? else {
                    return Err(RuntimeError::Unavailable);
                };
                Ok(json!({
                    "grant_id": grant.id,
                    "skill_id": grant.skill_id,
                    "display_name": skill.display_name,
                    "kind": skill.kind,
                    "available": skill_is_available(character, grant, skill, clock),
                    "costs": skill.costs,
                    "target": skill.target,
                    "cooldown_ticks": skill.cooldown_ticks,
                    "ready_at": grant.ready_at,
                }))
            })
            .collect::<Result<Vec<_>, RuntimeError>>()?;
        let next_after = has_more
            .then(|| skills.last().map(|grant| grant.id))
            .flatten();
        Ok(json!({
            "revision": context.revision,
            "skills": summaries,
            "next_after": next_after,
        }))
    }

    async fn inspect_skill(
        &self,
        context: &AgentToolContext,
        grant_id: ObjectId,
    ) -> Result<JsonValue, RuntimeError> {
        let inner = self.inner.lock().await;
        require_revision(&inner, context.revision)?;
        let character = inner
            .world
            .character(context.actor_id)
            .ok_or(RuntimeError::Unavailable)?;
        let records = inner.world.project_records()?;
        let grant = records
            .iter()
            .find_map(|record| match record {
                DomainRecord::SkillGrant(grant)
                    if grant.id == grant_id && grant.owner_id == context.actor_id =>
                {
                    Some(grant)
                }
                _ => None,
            })
            .ok_or(RuntimeError::Unavailable)?;
        let Definition::Skill(skill) = definition(&inner.registry, &grant.skill_id)? else {
            return Err(RuntimeError::Unavailable);
        };
        Ok(json!({
            "revision": context.revision,
            "grant": grant,
            "definition": skill,
            "available": skill_is_available(character, grant, skill, inner.world.world_state().clock),
        }))
    }

    pub async fn append_transcript(
        &self,
        actor_id: ActorId,
        session_id: SessionId,
        speaker: loreloom_core::TranscriptSpeaker,
        text: LongText,
        supporting_events: Vec<EventId>,
    ) -> Result<loreloom_core::TranscriptItemRecord, RuntimeError> {
        let mut inner = self.inner.lock().await;
        let revision = inner.world.revision().next()?;
        let transcript = loreloom_core::TranscriptItemRecord {
            id: loreloom_core::TranscriptItemId::generate_with(&mut inner.ids)?,
            session_id,
            revision: Some(revision),
            speaker,
            text,
            state: loreloom_core::TranscriptState::Committed,
            supporting_events,
        };
        let command = WorldCommand {
            action_id: ActionId::generate_with(&mut inner.ids)?,
            actor_id,
            expected_revision: inner.world.revision(),
            kind: WorldCommandKind::AppendTranscript {
                items: vec![transcript.clone()],
            },
        };
        apply_command(&mut inner, command).await?;
        Ok(transcript)
    }

    pub async fn execute(
        &self,
        context: &AgentToolContext,
        kind: WorldCommandKind,
    ) -> Result<CommittedAction, RuntimeError> {
        let mut inner = self.inner.lock().await;
        let command = WorldCommand {
            action_id: ActionId::generate_with(&mut inner.ids)?,
            actor_id: context.actor_id,
            expected_revision: context.revision,
            kind,
        };
        apply_command(&mut inner, command).await
    }

    async fn list_gameplay_actions(
        &self,
        context: &AgentToolContext,
    ) -> Result<JsonValue, RuntimeError> {
        let inner = self.inner.lock().await;
        if inner.world.revision() != context.revision {
            return Err(RuntimeError::Unavailable);
        }
        let mut actions = inner
            .registry
            .iter()
            .filter_map(|(_, entry)| match &entry.definition {
                Definition::GameplayAction(action)
                    if context.capabilities.contains(action.capability.as_str()) =>
                {
                    Some(json!({
                        "action_id": action.id,
                        "display_name": action.display_name,
                        "capability": action.capability,
                        "parameters": action.parameters,
                    }))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        actions.sort_by(|left, right| left["action_id"].as_str().cmp(&right["action_id"].as_str()));
        Ok(json!({
            "revision": context.revision,
            "actions": actions,
        }))
    }

    async fn list_scene_transitions(
        &self,
        context: &AgentToolContext,
    ) -> Result<JsonValue, RuntimeError> {
        let inner = self.inner.lock().await;
        if inner.world.revision() != context.revision {
            return Err(RuntimeError::Unavailable);
        }
        if inner.world.world_state().player_actor != context.actor_id
            || !context
                .capabilities
                .contains(NARRATOR_TRANSITION_SCENE_CAPABILITY)
        {
            return Err(RuntimeError::CapabilityDenied);
        }
        let projection = scene_transition_projection(&inner)?;
        Ok(json!({
            "revision": context.revision,
            "current": {
                "scene_id": projection.current.id,
                "scene_definition_id": projection.current.origin.content().map(|origin| &origin.definition_id),
                "display_name": projection.current.display_name,
                "framing": projection.current.framing,
            },
            "targets": projection.targets.into_iter().map(|option| json!({
                "target": option.target,
                "display_name": option.display_name,
                "framing": option.framing,
            })).collect::<Vec<_>>(),
        }))
    }

    async fn validate_scene_transition_target(
        &self,
        context: &AgentToolContext,
        target: &SceneTransitionTarget,
    ) -> Result<(), RuntimeError> {
        let inner = self.inner.lock().await;
        if inner.world.revision() != context.revision {
            return Err(RuntimeError::Unavailable);
        }
        let projection = scene_transition_projection(&inner)?;
        projection
            .targets
            .iter()
            .any(|option| &option.target == target)
            .then_some(())
            .ok_or(RuntimeError::SceneTransitionTargetUnavailable)
    }

    async fn perform_gameplay_action(
        &self,
        context: &AgentToolContext,
        action_id: ContentDefinitionId,
        arguments: BTreeMap<ContentDefinitionId, ParameterValue>,
    ) -> Result<CommittedAction, RuntimeError> {
        let mut inner = self.inner.lock().await;
        let capability = inner
            .registry
            .get(&action_id)
            .and_then(|entry| match &entry.definition {
                Definition::GameplayAction(action) => Some(action.capability.as_str()),
                _ => None,
            })
            .ok_or(RuntimeError::InvalidInput)?;
        if !context.capabilities.contains(capability) {
            return Err(RuntimeError::CapabilityDenied);
        }
        let command = WorldCommand {
            action_id: ActionId::generate_with(&mut inner.ids)?,
            actor_id: context.actor_id,
            expected_revision: context.revision,
            kind: WorldCommandKind::PerformGameplayAction {
                action_id,
                arguments,
            },
        };
        apply_command(&mut inner, command).await
    }

    pub async fn snapshot(
        &self,
        session_id: SessionId,
        phase: RuntimePhase,
        tool_activity: Vec<ToolActivity>,
        notices: Vec<UiNotice>,
        supporting_events: Vec<EventId>,
    ) -> Result<UiSnapshot, RuntimeError> {
        let inner = self.inner.lock().await;
        let revision = inner.world.revision();
        let player_id = inner.world.world_state().player_actor;
        let records = inner.world.project_records()?;
        Ok(UiSnapshot {
            revision,
            session_id,
            player: character_context(&records, &inner.registry, player_id, revision)?,
            scene: scene_context(
                &records,
                &inner.events,
                &inner.registry,
                player_id,
                revision,
            )?,
            parameters: parameter_views(
                &records,
                inner.world.session_parameters(),
                &inner.registry,
            )?,
            active_events: active_event_views(&records, &inner.registry, player_id)?,
            packages: package_catalog(inner.store.manifest()),
            transcript: transcript_window(&records, UI_TRANSCRIPT_LIMIT),
            tool_activity,
            phase,
            can_submit: matches!(phase, RuntimePhase::Idle | RuntimePhase::Completed),
            can_cancel: !matches!(
                phase,
                RuntimePhase::Idle
                    | RuntimePhase::Completed
                    | RuntimePhase::Cancelled
                    | RuntimePhase::Failed
            ),
            waiting: !matches!(
                phase,
                RuntimePhase::Idle
                    | RuntimePhase::Completed
                    | RuntimePhase::Cancelled
                    | RuntimePhase::Failed
            ),
            notices,
            supporting_events,
        })
    }
}

fn package_catalog(manifest: &SaveManifest) -> PackageCatalogView {
    let mut mods = manifest
        .mod_lock
        .mods
        .iter()
        .map(|locked| ModPackageView {
            mod_id: locked.mod_id.clone(),
            version: locked.version.clone(),
            status: ModPackageStatus::Enabled,
            dependency_count: u32::try_from(locked.dependencies.len()).unwrap_or(u32::MAX),
            content: Default::default(),
        })
        .collect::<Vec<_>>();
    mods.sort_by(|left, right| {
        left.mod_id
            .cmp(&right.mod_id)
            .then_with(|| left.version.cmp(&right.version))
    });
    PackageCatalogView {
        world: WorldPackageView {
            world_id: manifest.world_lock.world_id.clone(),
            version: manifest.world_lock.version.clone(),
        },
        mods,
        unavailable_installed: 0,
    }
}

#[derive(Debug)]
pub struct RuntimeToolExecutor {
    service: Arc<WorldService>,
    generation_policy: Option<GenerationPolicy>,
    pending_npc_decisions: Mutex<Vec<PendingNpcDecision>>,
    pending_npc_turns: Mutex<Vec<NpcTurnRequest>>,
    pending_npc_drafts: Mutex<Vec<NpcDraft>>,
    pending_topology: Mutex<Option<PendingTopologyRequest>>,
    request_ids: Mutex<SystemIdGenerator>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingNpcDecision {
    pub call_id: String,
    pub revision: Revision,
    pub decision: NarratorNpcDecision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingTopologyRequest {
    pub call_id: String,
    pub revision: Revision,
    pub kind: PendingTopologyKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PendingTopologyKind {
    Transition {
        target: SceneTransitionTarget,
    },
    CreateScene {
        display_name: DisplayName,
        framing: ShortText,
        entry_place_name: DisplayName,
        entry_place_description: ShortText,
    },
    CreatePlace {
        display_name: DisplayName,
        description: ShortText,
    },
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct NpcTurnToolRequest {
    actor_id: ActorId,
    assignment: AssignmentText,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateSceneToolRequest {
    display_name: DisplayName,
    framing: ShortText,
    entry_place_name: DisplayName,
    entry_place_description: ShortText,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CreatePlaceToolRequest {
    display_name: DisplayName,
    description: ShortText,
}

impl RuntimeToolExecutor {
    #[must_use]
    pub fn new(service: Arc<WorldService>) -> Self {
        Self::with_generation_policy(service, None)
    }

    #[must_use]
    pub fn with_generation_policy(
        service: Arc<WorldService>,
        generation_policy: Option<GenerationPolicy>,
    ) -> Self {
        Self {
            service,
            generation_policy,
            pending_npc_decisions: Mutex::new(Vec::new()),
            pending_npc_turns: Mutex::new(Vec::new()),
            pending_npc_drafts: Mutex::new(Vec::new()),
            pending_topology: Mutex::new(None),
            request_ids: Mutex::new(SystemIdGenerator),
        }
    }

    pub(crate) async fn take_pending_npc_decisions(&self) -> Vec<PendingNpcDecision> {
        std::mem::take(&mut *self.pending_npc_decisions.lock().await)
    }

    pub(crate) async fn take_pending_npc_turns(&self) -> Vec<NpcTurnRequest> {
        std::mem::take(&mut *self.pending_npc_turns.lock().await)
    }

    pub(crate) async fn take_pending_npc_drafts(&self) -> Vec<NpcDraft> {
        std::mem::take(&mut *self.pending_npc_drafts.lock().await)
    }

    pub(crate) async fn take_pending_topology(&self) -> Option<PendingTopologyRequest> {
        self.pending_topology.lock().await.take()
    }

    async fn queue_topology(
        &self,
        call_id: &str,
        revision: Revision,
        kind: PendingTopologyKind,
    ) -> Result<JsonValue, RuntimeError> {
        if !self.pending_npc_decisions.lock().await.is_empty()
            || !self.pending_npc_turns.lock().await.is_empty()
        {
            return Err(RuntimeError::WorldTopologyConflict);
        }
        let mut pending = self.pending_topology.lock().await;
        match pending.as_ref() {
            Some(existing) if existing.kind == kind => Ok(json!({
                "status": "accepted_pending",
                "revision": revision,
                "duplicate": true
            })),
            Some(_) => Err(RuntimeError::WorldTopologyConflict),
            None => {
                *pending = Some(PendingTopologyRequest {
                    call_id: call_id.to_owned(),
                    revision,
                    kind,
                });
                Ok(json!({
                    "status": "accepted_pending",
                    "revision": revision
                }))
            }
        }
    }
}

impl ToolExecutor for RuntimeToolExecutor {
    fn definitions(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "inspect_character_status".to_owned(),
                description: "Inspect the authorized actor at the bound revision.".to_owned(),
                input_schema: json!({
                    "type": "object",
                    "additionalProperties": false
                }),
            },
            ToolDefinition {
                name: "list_inventory".to_owned(),
                description: "List the authorized actor's inventory using a stable item-ID cursor."
                    .to_owned(),
                input_schema: page_schema(),
            },
            ToolDefinition {
                name: "inspect_item".to_owned(),
                description: "Inspect one item instance owned by the authorized actor.".to_owned(),
                input_schema: json!({
                    "type": "object",
                    "required": ["item_id"],
                    "properties": { "item_id": { "type": "string" } },
                    "additionalProperties": false
                }),
            },
            ToolDefinition {
                name: "transfer_item".to_owned(),
                description: "Transfer one owned item into an authorized container.".to_owned(),
                input_schema: json!({
                    "type": "object",
                    "required": ["item_id", "container_id"],
                    "properties": {
                        "item_id": { "type": "string" },
                        "container_id": { "type": "string" }
                    },
                    "additionalProperties": false
                }),
            },
            ToolDefinition {
                name: "equip_item".to_owned(),
                description: "Equip one owned item into a compatible equipment slot.".to_owned(),
                input_schema: json!({
                    "type": "object",
                    "required": ["item_id", "slot_id"],
                    "properties": {
                        "item_id": { "type": "string" },
                        "slot_id": { "type": "string" }
                    },
                    "additionalProperties": false
                }),
            },
            ToolDefinition {
                name: "split_stack".to_owned(),
                description: "Split a positive quantity from one owned item stack.".to_owned(),
                input_schema: json!({
                    "type": "object",
                    "required": ["item_id", "quantity"],
                    "properties": {
                        "item_id": { "type": "string" },
                        "quantity": { "type": "integer", "minimum": 1, "maximum": u32::MAX }
                    },
                    "additionalProperties": false
                }),
            },
            ToolDefinition {
                name: "list_available_skills".to_owned(),
                description: "List the authorized actor's skill grants, readiness, costs and targets using a stable grant-ID cursor.".to_owned(),
                input_schema: page_schema(),
            },
            ToolDefinition {
                name: "inspect_skill".to_owned(),
                description: "Inspect one skill grant owned by the authorized actor.".to_owned(),
                input_schema: json!({
                    "type": "object",
                    "required": ["grant_id"],
                    "properties": { "grant_id": { "type": "string" } },
                    "additionalProperties": false
                }),
            },
            ToolDefinition {
                name: "use_skill".to_owned(),
                description: "Use one active skill grant owned by the authorized actor.".to_owned(),
                input_schema: json!({
                    "type": "object",
                    "required": ["grant_id", "target"],
                    "properties": {
                        "grant_id": { "type": "string" },
                        "target": {
                            "oneOf": [
                                {
                                    "type": "object",
                                    "required": ["type"],
                                    "properties": { "type": { "const": "self_target" } },
                                    "additionalProperties": false
                                },
                                {
                                    "type": "object",
                                    "required": ["type", "object_id"],
                                    "properties": {
                                        "type": { "const": "object" },
                                        "object_id": { "type": "string" }
                                    },
                                    "additionalProperties": false
                                },
                                {
                                    "type": "object",
                                    "required": ["type", "place_id"],
                                    "properties": {
                                        "type": { "const": "place" },
                                        "place_id": { "type": "string" }
                                    },
                                    "additionalProperties": false
                                }
                            ]
                        }
                    },
                    "additionalProperties": false
                }),
            },
            ToolDefinition {
                name: "move_character".to_owned(),
                description: "Move the authorized actor to an adjacent place.".to_owned(),
                input_schema: json!({
                    "type": "object",
                    "required": ["destination_id"],
                    "properties": { "destination_id": { "type": "string" } },
                    "additionalProperties": false
                }),
            },
            ToolDefinition {
                name: "advance_time".to_owned(),
                description: "Explicitly advance the logical World clock.".to_owned(),
                input_schema: json!({
                    "type": "object",
                    "required": ["ticks"],
                    "properties": { "ticks": { "type": "integer", "minimum": 1 } },
                    "additionalProperties": false
                }),
            },
            ToolDefinition {
                name: "list_gameplay_actions".to_owned(),
                description: "List declarative gameplay actions authorized for this agent."
                    .to_owned(),
                input_schema: json!({
                    "type": "object",
                    "additionalProperties": false
                }),
            },
            ToolDefinition {
                name: "perform_gameplay_action".to_owned(),
                description: "Perform one authorized declarative gameplay action.".to_owned(),
                input_schema: json!({
                    "type": "object",
                    "required": ["action_id", "arguments"],
                    "properties": {
                        "action_id": { "type": "string" },
                        "arguments": {
                            "type": "object",
                            "additionalProperties": {
                                "type": "object",
                                "required": ["type", "value"]
                            }
                        }
                    },
                    "additionalProperties": false
                }),
            },
            ToolDefinition {
                name: "choose_event_option".to_owned(),
                description: "Choose an option on an active event instance.".to_owned(),
                input_schema: json!({
                    "type": "object",
                    "required": ["event_instance_id", "option_id"],
                    "properties": {
                        "event_instance_id": { "type": "string" },
                        "option_id": { "type": "string" }
                    },
                    "additionalProperties": false
                }),
            },
            ToolDefinition {
                name: "submit_npc_draft".to_owned(),
                description: "Submit one generated NPC draft for Runtime validation."
                    .to_owned(),
                input_schema: npc_draft_schema(),
            },
            ToolDefinition {
                name: "request_npc_turn".to_owned(),
                description: "Queue one turn for an existing visible NPC whose observation has npc_turn_available=true. Copy its actor_id exactly; the runtime supplies scene and revision.".to_owned(),
                input_schema: json!({
                    "type": "object",
                    "required": ["actor_id", "assignment"],
                    "properties": {
                        "actor_id": { "type": "string" },
                        "assignment": { "type": "string" }
                    },
                    "additionalProperties": false
                }),
            },
            ToolDefinition {
                name: "list_scene_transitions".to_owned(),
                description: "List the current scene and exact canonical transition targets available at this revision. Call this before transition_scene and copy one returned target object without changing it.".to_owned(),
                input_schema: json!({
                    "type": "object",
                    "additionalProperties": false
                }),
            },
            ToolDefinition {
                name: "transition_scene".to_owned(),
                description: "Transition after this narrator turn using one exact target object returned by list_scene_transitions. Never invent a scene ID or narrate arrival before the committed transition result.".to_owned(),
                input_schema: json!({
                    "type": "object",
                    "required": ["target"],
                    "properties": {
                        "target": {
                            "oneOf": [
                                {
                                    "type": "object",
                                    "required": ["type", "scene_id"],
                                    "properties": {
                                        "type": { "const": "existing" },
                                        "scene_id": { "type": "string" }
                                    },
                                    "additionalProperties": false
                                },
                                {
                                    "type": "object",
                                    "required": ["type", "scene_definition_id"],
                                    "properties": {
                                        "type": { "const": "definition" },
                                        "scene_definition_id": { "type": "string" }
                                    },
                                    "additionalProperties": false
                                }
                            ]
                        }
                    },
                    "additionalProperties": false
                }),
            },
            ToolDefinition {
                name: "create_scene".to_owned(),
                description: "Create a persistent inactive scene and its entry place after this narrator turn. The runtime supplies IDs and provenance; replan before transitioning.".to_owned(),
                input_schema: json!({
                    "type": "object",
                    "required": [
                        "display_name",
                        "framing",
                        "entry_place_name",
                        "entry_place_description"
                    ],
                    "properties": {
                        "display_name": { "type": "string", "maxLength": 256 },
                        "framing": { "type": "string", "maxLength": 4096 },
                        "entry_place_name": { "type": "string", "maxLength": 256 },
                        "entry_place_description": { "type": "string", "maxLength": 4096 }
                    },
                    "additionalProperties": false
                }),
            },
            ToolDefinition {
                name: "create_place".to_owned(),
                description: "Create a persistent place in the active scene after this narrator turn. The runtime connects it to the current place and supplies IDs and provenance; replan before moving.".to_owned(),
                input_schema: json!({
                    "type": "object",
                    "required": ["display_name", "description"],
                    "properties": {
                        "display_name": { "type": "string", "maxLength": 256 },
                        "description": { "type": "string", "maxLength": 4096 }
                    },
                    "additionalProperties": false
                }),
            },
            ToolDefinition {
                name: "create_npc".to_owned(),
                description: "Create a preset or generated NPC in the current place after this narrator turn. Planning restarts with the committed actor; request its turn separately only after it appears in observation.".to_owned(),
                input_schema: json!({
                    "type": "object",
                    "required": ["source", "lifetime", "mode"],
                    "properties": {
                        "source": {
                            "oneOf": [
                                {
                                    "type": "object",
                                    "required": ["type", "character_id"],
                                    "properties": {
                                        "type": { "const": "preset" },
                                        "character_id": { "type": "string" }
                                    },
                                    "additionalProperties": false
                                },
                                {
                                    "type": "object",
                                    "required": ["type", "role", "purpose"],
                                    "properties": {
                                        "type": { "const": "generated" },
                                        "role": { "type": "string", "maxLength": 1024 },
                                        "purpose": { "type": "string", "maxLength": 65536 }
                                    },
                                    "additionalProperties": false
                                }
                            ]
                        },
                        "lifetime": {
                            "type": "string",
                            "enum": ["scene", "persistent"]
                        },
                        "mode": {
                            "type": "string",
                            "enum": ["narrated", "agent"]
                        }
                    },
                    "additionalProperties": false
                }),
            },
        ]
    }

    fn execute<'a>(
        &'a self,
        context: ToolContext,
        call: ToolCall,
    ) -> BoxFuture<'a, Result<ToolResult, ToolExecutionError>> {
        Box::pin(async move {
            let runtime = context.get::<AgentToolContext>().ok_or_else(|| {
                ToolExecutionError::ExecutionFailed {
                    name: call.name.clone(),
                    message: "missing runtime context".to_owned(),
                }
            })?;
            let result = match call.name.as_str() {
                "inspect_character_status" => self
                    .service
                    .inspect_character(runtime.actor_id, runtime.revision)
                    .await
                    .map(|character| json!({ "character": character })),
                "list_inventory" => match parse_page_arguments(&call.arguments) {
                    Ok((after, limit)) => self.service.list_inventory(runtime, after, limit).await,
                    Err(error) => Err(error),
                },
                "inspect_item" => match parse_object_id(&call.arguments, "item_id") {
                    Ok(item_id) => self.service.inspect_item(runtime, item_id).await,
                    Err(error) => Err(error),
                },
                "transfer_item" => match (
                    parse_object_id(&call.arguments, "item_id"),
                    parse_object_id(&call.arguments, "container_id"),
                ) {
                    (Ok(item_id), Ok(container_id)) => self
                        .service
                        .execute(
                            runtime,
                            WorldCommandKind::TransferItem {
                                item_id,
                                container_id,
                            },
                        )
                        .await
                        .map(committed_json),
                    _ => Err(RuntimeError::InvalidInput),
                },
                "equip_item" => match (
                    parse_object_id(&call.arguments, "item_id"),
                    parse_definition_id(&call.arguments, "slot_id"),
                ) {
                    (Ok(item_id), Ok(slot_id)) => self
                        .service
                        .execute(runtime, WorldCommandKind::EquipItem { item_id, slot_id })
                        .await
                        .map(committed_json),
                    _ => Err(RuntimeError::InvalidInput),
                },
                "split_stack" => match (
                    parse_object_id(&call.arguments, "item_id"),
                    parse_u32(&call.arguments, "quantity"),
                ) {
                    (Ok(item_id), Ok(quantity)) if quantity > 0 => self
                        .service
                        .execute(runtime, WorldCommandKind::SplitStack { item_id, quantity })
                        .await
                        .map(committed_json),
                    _ => Err(RuntimeError::InvalidInput),
                },
                "list_available_skills" => match parse_page_arguments(&call.arguments) {
                    Ok((after, limit)) => {
                        self.service
                            .list_available_skills(runtime, after, limit)
                            .await
                    }
                    Err(error) => Err(error),
                },
                "inspect_skill" => match parse_object_id(&call.arguments, "grant_id") {
                    Ok(grant_id) => self.service.inspect_skill(runtime, grant_id).await,
                    Err(error) => Err(error),
                },
                "use_skill" => match (
                    parse_object_id(&call.arguments, "grant_id"),
                    parse_skill_target(&call.arguments),
                ) {
                    (Ok(grant_id), Ok(target)) => self
                        .service
                        .execute(runtime, WorldCommandKind::UseSkill { grant_id, target })
                        .await
                        .map(committed_json),
                    _ => Err(RuntimeError::InvalidInput),
                },
                "move_character" => match parse_object_id(&call.arguments, "destination_id") {
                    Ok(destination_id) => self
                        .service
                        .execute(runtime, WorldCommandKind::Move { destination_id })
                        .await
                        .map(committed_json),
                    Err(error) => Err(error),
                },
                "advance_time" => match call.arguments.get("ticks").and_then(JsonValue::as_u64) {
                    Some(ticks) if ticks > 0 => self
                        .service
                        .execute(runtime, WorldCommandKind::AdvanceTime { ticks })
                        .await
                        .map(committed_json),
                    _ => Err(RuntimeError::InvalidInput),
                },
                "list_gameplay_actions" => self.service.list_gameplay_actions(runtime).await,
                "perform_gameplay_action" => match parse_gameplay_action(&call.arguments) {
                    Ok((action_id, arguments)) => self
                        .service
                        .perform_gameplay_action(runtime, action_id, arguments)
                        .await
                        .map(committed_json),
                    Err(error) => Err(error),
                },
                "choose_event_option" => match (
                    parse_object_id(&call.arguments, "event_instance_id"),
                    parse_definition_id(&call.arguments, "option_id"),
                ) {
                    (Ok(event_instance_id), Ok(option_id)) => self
                        .service
                        .execute(
                            runtime,
                            WorldCommandKind::ChooseEventOption {
                                event_instance_id,
                                option_id,
                            },
                        )
                        .await
                        .map(committed_json),
                    _ => Err(RuntimeError::InvalidInput),
                },
                "create_scene" => {
                    if !runtime
                        .capabilities
                        .contains(NARRATOR_CREATE_SCENE_CAPABILITY)
                        || self.service.player_actor().await != runtime.actor_id
                    {
                        Err(RuntimeError::CapabilityDenied)
                    } else if self.service.revision().await != runtime.revision {
                        Err(RuntimeError::Unavailable)
                    } else {
                        match serde_json::from_value::<CreateSceneToolRequest>(
                            call.arguments.clone(),
                        ) {
                            Ok(request) => {
                                self.queue_topology(
                                    call.id.as_str(),
                                    runtime.revision,
                                    PendingTopologyKind::CreateScene {
                                        display_name: request.display_name,
                                        framing: request.framing,
                                        entry_place_name: request.entry_place_name,
                                        entry_place_description: request.entry_place_description,
                                    },
                                )
                                .await
                            }
                            Err(_) => Err(RuntimeError::InvalidInput),
                        }
                    }
                }
                "create_place" => {
                    if !runtime
                        .capabilities
                        .contains(NARRATOR_CREATE_PLACE_CAPABILITY)
                        || self.service.player_actor().await != runtime.actor_id
                    {
                        Err(RuntimeError::CapabilityDenied)
                    } else if self.service.revision().await != runtime.revision {
                        Err(RuntimeError::Unavailable)
                    } else {
                        match serde_json::from_value::<CreatePlaceToolRequest>(
                            call.arguments.clone(),
                        ) {
                            Ok(request) => {
                                self.queue_topology(
                                    call.id.as_str(),
                                    runtime.revision,
                                    PendingTopologyKind::CreatePlace {
                                        display_name: request.display_name,
                                        description: request.description,
                                    },
                                )
                                .await
                            }
                            Err(_) => Err(RuntimeError::InvalidInput),
                        }
                    }
                }
                "create_npc" => {
                    if !runtime
                        .capabilities
                        .contains(NARRATOR_CREATE_NPC_CAPABILITY)
                        || self.service.player_actor().await != runtime.actor_id
                    {
                        Err(RuntimeError::CapabilityDenied)
                    } else if self.service.revision().await != runtime.revision {
                        Err(RuntimeError::Unavailable)
                    } else if self.pending_topology.lock().await.is_some() {
                        Err(RuntimeError::WorldTopologyConflict)
                    } else {
                        async {
                            let request =
                                serde_json::from_value::<CreateNpcRequest>(call.arguments.clone())
                                    .map_err(|_| RuntimeError::InvalidInput)?;
                            let (scene_id, place_id) = self
                                .service
                                .npc_creation_location(runtime.actor_id, runtime.revision)
                                .await?;
                            let action = match request.mode {
                                NpcCreationMode::Narrated => {
                                    NpcNarrativeAction::MaterializeLightweight
                                }
                                NpcCreationMode::Agent => NpcNarrativeAction::RequestNpcTurn,
                            };
                            let (target, controller) = match request.source {
                                NpcCreationSource::Preset { character_id } => {
                                    let controller = match request.mode {
                                        NpcCreationMode::Narrated => {
                                            NpcControllerKind::NarratorProxy
                                        }
                                        NpcCreationMode::Agent => NpcControllerKind::Agent(
                                            self.service
                                                .preset_agent_profile(&character_id)
                                                .await?,
                                        ),
                                    };
                                    (
                                        NpcTarget::Preset {
                                            character_id,
                                            place_id,
                                        },
                                        controller,
                                    )
                                }
                                NpcCreationSource::Generated { role, purpose } => {
                                    let policy = self
                                        .generation_policy
                                        .as_ref()
                                        .ok_or(RuntimeError::Unavailable)?;
                                    let controller = match request.mode {
                                        NpcCreationMode::Narrated => {
                                            NpcControllerKind::NarratorProxy
                                        }
                                        NpcCreationMode::Agent => {
                                            let profile_id = policy
                                                .allowed_agent_profiles
                                                .iter()
                                                .next()
                                                .filter(|_| {
                                                    policy.allowed_agent_profiles.len() == 1
                                                })
                                                .cloned()
                                                .ok_or(RuntimeError::Unavailable)?;
                                            NpcControllerKind::Agent(profile_id)
                                        }
                                    };
                                    (
                                        NpcTarget::Generated {
                                            generation_policy_id: policy.id.clone(),
                                            place_id,
                                            request: NpcGenerationRequest {
                                                scene_id,
                                                role,
                                                purpose,
                                                desired_traits: BTreeSet::new(),
                                                importance: NarrativeImportance::Supporting,
                                            },
                                        },
                                        controller,
                                    )
                                }
                            };
                            let decision = NarratorNpcDecision {
                                target,
                                action,
                                lifetime: request.lifetime,
                                controller,
                                assignment: None,
                            };
                            decision.validate()?;
                            self.pending_npc_decisions
                                .lock()
                                .await
                                .push(PendingNpcDecision {
                                    call_id: call.id.as_str().to_owned(),
                                    revision: runtime.revision,
                                    decision,
                                });
                            Ok::<JsonValue, RuntimeError>(json!({
                                "status": "accepted_pending",
                                "revision": runtime.revision,
                                "requires_replanning_after_materialization": true
                            }))
                        }
                        .await
                    }
                }
                "request_npc_turn" => {
                    if !runtime
                        .capabilities
                        .contains(NARRATOR_REQUEST_NPC_TURN_CAPABILITY)
                        || self.service.player_actor().await != runtime.actor_id
                    {
                        Err(RuntimeError::CapabilityDenied)
                    } else if self.service.revision().await != runtime.revision {
                        Err(RuntimeError::Unavailable)
                    } else if self.pending_topology.lock().await.is_some() {
                        Err(RuntimeError::WorldTopologyConflict)
                    } else {
                        async {
                            let request = serde_json::from_value::<NpcTurnToolRequest>(
                                call.arguments.clone(),
                            )
                            .map_err(|_| RuntimeError::InvalidInput)?;
                            let scene_id = self
                                .service
                                .npc_turn_scene(
                                    runtime.actor_id,
                                    request.actor_id,
                                    runtime.revision,
                                )
                                .await?;
                            let request_id = NpcTurnRequestId::generate_with(
                                &mut *self.request_ids.lock().await,
                            )?;
                            self.pending_npc_turns.lock().await.push(NpcTurnRequest {
                                request_id,
                                actor_id: request.actor_id,
                                scene_id,
                                based_on_revision: runtime.revision,
                                assignment: request.assignment,
                            });
                            Ok::<JsonValue, RuntimeError>(json!({
                                "status": "accepted_pending",
                                "request_id": request_id,
                                "revision": runtime.revision
                            }))
                        }
                        .await
                    }
                }
                "list_scene_transitions" => self.service.list_scene_transitions(runtime).await,
                "transition_scene" => {
                    if !runtime
                        .capabilities
                        .contains(NARRATOR_TRANSITION_SCENE_CAPABILITY)
                        || self.service.player_actor().await != runtime.actor_id
                    {
                        Err(RuntimeError::CapabilityDenied)
                    } else if self.service.revision().await != runtime.revision {
                        Err(RuntimeError::Unavailable)
                    } else {
                        let target = match call
                            .arguments
                            .get("target")
                            .cloned()
                            .ok_or(RuntimeError::SceneTransitionTargetUnavailable)
                            .and_then(|value| {
                                serde_json::from_value::<SceneTransitionTarget>(value)
                                    .map_err(|_| RuntimeError::SceneTransitionTargetUnavailable)
                            }) {
                            Ok(target) => target,
                            Err(error) => {
                                return Ok(tool_result(&call, runtime_error_json(&error), true));
                            }
                        };
                        match self
                            .service
                            .validate_scene_transition_target(runtime, &target)
                            .await
                        {
                            Ok(()) => {
                                self.queue_topology(
                                    call.id.as_str(),
                                    runtime.revision,
                                    PendingTopologyKind::Transition { target },
                                )
                                .await
                            }
                            Err(error) => Err(error),
                        }
                    }
                }
                "submit_npc_draft" => {
                    if !runtime
                        .capabilities
                        .contains(NARRATOR_SUBMIT_NPC_DRAFT_CAPABILITY)
                        || self.service.player_actor().await != runtime.actor_id
                    {
                        Err(RuntimeError::CapabilityDenied)
                    } else if self.service.revision().await != runtime.revision {
                        Err(RuntimeError::Unavailable)
                    } else {
                        match serde_json::from_value::<NpcDraft>(call.arguments.clone()) {
                            Ok(draft) => {
                                let mut pending = self.pending_npc_drafts.lock().await;
                                if pending.is_empty() {
                                    pending.push(draft);
                                    Ok(json!({
                                        "status": "accepted_pending",
                                        "revision": runtime.revision
                                    }))
                                } else {
                                    Err(RuntimeError::InvalidInput)
                                }
                            }
                            Err(_) => Err(RuntimeError::InvalidInput),
                        }
                    }
                }
                _ => {
                    return Err(ToolExecutionError::UnknownTool {
                        name: call.name.clone(),
                    });
                }
            };
            Ok(match result {
                Ok(value) => tool_result(&call, value, false),
                Err(error) => tool_result(&call, runtime_error_json(&error), true),
            })
        })
    }
}

fn scene_transition_projection(
    inner: &RuntimeWorld,
) -> Result<SceneTransitionProjection, RuntimeError> {
    let active_scene = inner.world.world_state().active_scene;
    let scenes = inner
        .world
        .project_records()?
        .into_iter()
        .filter_map(|record| match record {
            DomainRecord::Scene(scene) => Some(scene),
            _ => None,
        })
        .collect::<Vec<_>>();
    let current = scenes
        .iter()
        .find(|scene| scene.id == active_scene && scene.active)
        .cloned()
        .ok_or(RuntimeError::Unavailable)?;
    let materialized_definitions = scenes
        .iter()
        .filter_map(|scene| {
            scene
                .origin
                .content()
                .map(|origin| origin.definition_id.clone())
        })
        .collect::<BTreeSet<_>>();
    let mut targets = scenes
        .into_iter()
        .filter(|scene| scene.id != active_scene && !scene.active)
        .map(|scene| SceneTransitionOption {
            target: SceneTransitionTarget::Existing { scene_id: scene.id },
            display_name: scene.display_name,
            framing: scene.framing,
        })
        .collect::<Vec<_>>();
    targets.extend(inner.registry.iter().filter_map(|(_, entry)| {
        let Definition::Scene(scene) = &entry.definition else {
            return None;
        };
        (!materialized_definitions.contains(&scene.id)).then(|| SceneTransitionOption {
            target: SceneTransitionTarget::Definition {
                scene_definition_id: scene.id.clone(),
            },
            display_name: scene.display_name.clone(),
            framing: scene.framing.clone(),
        })
    }));
    targets.sort_by_key(|option| match &option.target {
        SceneTransitionTarget::Existing { scene_id } => format!("existing:{scene_id}"),
        SceneTransitionTarget::Definition {
            scene_definition_id,
        } => format!("definition:{scene_definition_id}"),
    });
    Ok(SceneTransitionProjection { current, targets })
}

async fn spawn_character(
    inner: &mut RuntimeWorld,
    acting_actor: ActorId,
    spec: CharacterSpawnSpec,
) -> Result<ActorId, RuntimeError> {
    let command = WorldCommand {
        action_id: ActionId::generate_with(&mut inner.ids)?,
        actor_id: acting_actor,
        expected_revision: inner.world.revision(),
        kind: WorldCommandKind::SpawnCharacter {
            spec: Box::new(spec),
        },
    };
    let outcome = apply_command(inner, command).await?;
    inner
        .events
        .iter()
        .filter(|event| outcome.event_ids.contains(&event.id))
        .find_map(|event| match event.kind {
            WorldEventKind::CharacterSpawned { character_id } => Some(character_id),
            _ => None,
        })
        .ok_or(RuntimeError::Unavailable)
}

async fn apply_command(
    inner: &mut RuntimeWorld,
    command: WorldCommand,
) -> Result<CommittedAction, RuntimeError> {
    let committed_session = inner.world.session_parameters().clone();
    let changes = {
        let RuntimeWorld {
            world,
            registry,
            ids,
            ..
        } = inner;
        match world.execute(command.clone(), registry, ids) {
            Ok(changes) => changes,
            Err(error) => {
                recover(inner, committed_session).await?;
                return Err(RuntimeError::World(error));
            }
        }
    };
    let candidate_session = inner.world.session_parameters().clone();
    let events = changes.events.clone();
    let request = CommitRequest::from_execution(command.clone(), changes)?;
    match inner.store.commit(&request).await {
        Ok(CommitResult::Committed(outcome) | CommitResult::AlreadyCommitted(outcome)) => {
            for event in events {
                if !inner.events.iter().any(|existing| existing.id == event.id) {
                    inner.events.push(event);
                }
            }
            inner.events.sort_by_key(|event| (event.revision, event.id));
            Ok(outcome)
        }
        Ok(CommitResult::Conflict { .. }) => {
            recover(inner, committed_session).await?;
            Err(RuntimeError::Unavailable)
        }
        Ok(CommitResult::ActionIdentityConflict { .. }) => {
            recover(inner, committed_session).await?;
            Err(RuntimeError::Unavailable)
        }
        Err(error) => {
            let resolution = inner.store.resolve_action(&command).await;
            match resolution {
                Ok(ActionResolution::Committed(outcome)) => {
                    recover(inner, candidate_session).await?;
                    Ok(outcome)
                }
                Ok(
                    ActionResolution::NotCommitted { .. }
                    | ActionResolution::ActionIdentityConflict { .. },
                )
                | Err(_) => {
                    recover(inner, committed_session).await?;
                    Err(RuntimeError::Store(error))
                }
            }
        }
    }
}

async fn recover(
    inner: &mut RuntimeWorld,
    session_parameters: BTreeMap<ContentDefinitionId, ParameterValue>,
) -> Result<(), RuntimeError> {
    let loaded = inner.store.load().await?;
    let mut world = GameWorld::from_records(
        loaded.revision,
        loaded.records,
        inner.config.clone(),
        &inner.registry,
    )?;
    world.restore_session_parameters(session_parameters, &inner.registry)?;
    inner.world = world;
    inner.events = loaded.events;
    Ok(())
}

fn character_context(
    records: &[DomainRecord],
    registry: &DefinitionRegistry,
    actor_id: ActorId,
    revision: Revision,
) -> Result<CharacterContext, RuntimeError> {
    let character = records
        .iter()
        .find_map(|record| match record {
            DomainRecord::Character(character) if character.id == actor_id => Some(character),
            _ => None,
        })
        .ok_or(RuntimeError::Unavailable)?;
    let clock = records
        .iter()
        .find_map(|record| match record {
            DomainRecord::WorldState(state) => Some(state.clock),
            _ => None,
        })
        .ok_or(RuntimeError::Unavailable)?;
    let attributes = attribute_views(records, registry, character)?;
    let mut resources = character
        .resources
        .values()
        .map(|resource| {
            let Definition::Resource(definition) = definition(registry, &resource.resource_id)?
            else {
                return Err(RuntimeError::Unavailable);
            };
            let maximum = definition
                .derived_from_attribute
                .as_ref()
                .and_then(|attribute_id| {
                    attributes
                        .iter()
                        .find(|attribute| &attribute.attribute_id == attribute_id)
                        .map(|attribute| attribute.effective)
                })
                .unwrap_or(resource.base_maximum);
            Ok(ResourceView {
                resource_id: resource.resource_id.clone(),
                display_name: definition.display_name.clone(),
                current: resource.current,
                maximum,
            })
        })
        .collect::<Result<Vec<_>, RuntimeError>>()?;
    resources.sort_by(|left, right| left.resource_id.cmp(&right.resource_id));
    let mut inventory = records
        .iter()
        .filter_map(|record| match record {
            DomainRecord::Item(item) if item.owned_by == Some(actor_id) => Some(item),
            _ => None,
        })
        .map(|item| {
            let Definition::Item(definition) = definition(registry, &item.definition_id)? else {
                return Err(RuntimeError::Unavailable);
            };
            Ok(InventoryView {
                item: item.clone(),
                display_name: definition.display_name.clone(),
            })
        })
        .collect::<Result<Vec<_>, RuntimeError>>()?;
    inventory.sort_by_key(|view| view.item.id);
    let mut skills = records
        .iter()
        .filter_map(|record| match record {
            DomainRecord::SkillGrant(grant) if grant.owner_id == actor_id => Some(grant),
            _ => None,
        })
        .map(|grant| {
            let Definition::Skill(definition) = definition(registry, &grant.skill_id)? else {
                return Err(RuntimeError::Unavailable);
            };
            Ok(SkillView {
                available: grant.enabled && grant.ready_at.is_none_or(|ready| ready <= clock),
                grant: grant.clone(),
                display_name: definition.display_name.clone(),
            })
        })
        .collect::<Result<Vec<_>, RuntimeError>>()?;
    skills.sort_by_key(|view| view.grant.id);
    let mut conditions = records
        .iter()
        .filter_map(|record| match record {
            DomainRecord::Condition(condition) if condition.target_id == actor_id => {
                Some(condition)
            }
            _ => None,
        })
        .map(|condition| {
            let Definition::Condition(definition) = definition(registry, &condition.condition_id)?
            else {
                return Err(RuntimeError::Unavailable);
            };
            Ok(ConditionView {
                condition: condition.clone(),
                display_name: condition_is_diagnosed(records, registry, actor_id, condition)?
                    .then(|| definition.display_name.clone()),
                symptoms: definition
                    .symptoms
                    .iter()
                    .filter(|symptom| symptom.minimum_intensity <= condition.intensity)
                    .map(|symptom| symptom.text.clone())
                    .collect(),
            })
        })
        .collect::<Result<Vec<_>, RuntimeError>>()?;
    conditions.sort_by_key(|view| view.condition.id);
    let mut known_facts = records
        .iter()
        .filter_map(|record| match record {
            DomainRecord::KnownFact(fact)
                if fact.owner_id == actor_id && fact.status != KnowledgeStatus::Forgotten =>
            {
                Some(fact.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    known_facts.sort_by_key(|fact| fact.id);
    let mut goals = records
        .iter()
        .filter_map(|record| match record {
            DomainRecord::Goal(goal) if goal.owner_id == actor_id => Some(goal.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    goals.sort_by_key(|goal| goal.id);
    Ok(CharacterContext {
        actor_id,
        revision,
        display_name: character.display_name.clone(),
        profile: character.profile.clone(),
        location_id: character.location,
        attributes,
        resources,
        conditions,
        inventory,
        skills,
        known_facts,
        goals,
        life_state: character.life_state,
        action_state: character.action_state,
        posture: character.posture,
    })
}

fn condition_is_diagnosed(
    records: &[DomainRecord],
    registry: &DefinitionRegistry,
    observer_id: ActorId,
    condition: &ConditionRecord,
) -> Result<bool, RuntimeError> {
    let predicate_id = ContentDefinitionId::parse(DIAGNOSED_CONDITION_PREDICATE_ID)
        .map_err(|_| RuntimeError::Unavailable)?;
    let diagnosed = records.iter().any(|record| match record {
        DomainRecord::KnownFact(fact) => {
            fact.owner_id == observer_id
                && fact.subject
                    == FactSubject::Object {
                        object_id: condition.target_id.object_id(),
                    }
                && fact.predicate_id == predicate_id
                && fact.value == FactValue::Tag(condition.condition_id.clone())
                && fact.status == KnowledgeStatus::Confirmed
        }
        _ => false,
    });
    if diagnosed {
        let Definition::Tag(_) = definition(registry, &predicate_id)? else {
            return Err(RuntimeError::Unavailable);
        };
    }
    Ok(diagnosed)
}

fn scene_context(
    records: &[DomainRecord],
    events: &[WorldEvent],
    registry: &DefinitionRegistry,
    actor_id: ActorId,
    revision: Revision,
) -> Result<SceneContext, RuntimeError> {
    let character = records
        .iter()
        .find_map(|record| match record {
            DomainRecord::Character(character) if character.id == actor_id => Some(character),
            _ => None,
        })
        .ok_or(RuntimeError::Unavailable)?;
    let place = records
        .iter()
        .find_map(|record| match record {
            DomainRecord::Place(place) if place.id == character.location => Some(place),
            _ => None,
        })
        .ok_or(RuntimeError::Unavailable)?;
    let scene = records
        .iter()
        .find_map(|record| match record {
            DomainRecord::Scene(scene) if scene.id == place.scene_id => Some(scene),
            _ => None,
        })
        .ok_or(RuntimeError::Unavailable)?;
    let state = records
        .iter()
        .find_map(|record| match record {
            DomainRecord::WorldState(state) => Some(state),
            _ => None,
        })
        .ok_or(RuntimeError::Unavailable)?;
    let mut visible_actors = records
        .iter()
        .filter_map(|record| match record {
            DomainRecord::Character(other) if other.location == character.location => {
                Some(VisibleActorView {
                    actor_id: other.id,
                    display_name: other.display_name.clone(),
                    controller: other.controller,
                    npc_turn_available: other.controller == CharacterController::Agent
                        && other.agent_binding.as_ref().is_some_and(|binding| {
                            binding.enabled
                                && matches!(
                                    registry
                                        .get(&binding.profile_id)
                                        .map(|entry| &entry.definition),
                                    Some(Definition::AgentProfile(_))
                                )
                        }),
                    life_state: other.life_state,
                    action_state: other.action_state,
                    posture: other.posture,
                })
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    visible_actors.sort_by_key(|actor| actor.actor_id);
    let mut adjacent_places = place
        .edges
        .iter()
        .map(|place_id| {
            records
                .iter()
                .find_map(|record| match record {
                    DomainRecord::Place(adjacent) if adjacent.id == *place_id => {
                        Some(AdjacentPlaceView {
                            place_id: adjacent.id,
                            display_name: adjacent.display_name.clone(),
                            description: adjacent.description.clone(),
                        })
                    }
                    _ => None,
                })
                .ok_or(RuntimeError::Unavailable)
        })
        .collect::<Result<Vec<_>, _>>()?;
    adjacent_places.sort_by_key(|adjacent| adjacent.place_id);
    Ok(SceneContext {
        scene_id: scene.id,
        revision,
        display_name: scene.display_name.clone(),
        framing: scene.framing.clone(),
        place_id: place.id,
        place_name: place.display_name.clone(),
        adjacent_places,
        clock: state.clock,
        visible_actors,
        recent_events: tail(events.iter().cloned(), CONTEXT_EVENT_LIMIT),
    })
}

fn definition<'a>(
    registry: &'a DefinitionRegistry,
    id: &loreloom_core::ContentDefinitionId,
) -> Result<&'a Definition, RuntimeError> {
    registry
        .get(id)
        .map(|registered| &registered.definition)
        .ok_or(RuntimeError::Unavailable)
}

fn attribute_views(
    records: &[DomainRecord],
    registry: &DefinitionRegistry,
    character: &loreloom_core::CharacterRecord,
) -> Result<Vec<AttributeView>, RuntimeError> {
    let mut adjustments = character.attribute_adjustments.clone();
    for record in records {
        match record {
            DomainRecord::Item(item)
                if item
                    .equipped
                    .as_ref()
                    .is_some_and(|equipped| equipped.wearer_id == character.id) =>
            {
                adjustments.extend(item.instance_adjustments.iter().cloned());
                let Definition::Item(item_definition) = definition(registry, &item.definition_id)?
                else {
                    return Err(RuntimeError::Unavailable);
                };
                adjustments.extend(item_definition.modifiers.iter().map(|modifier| {
                    AttributeAdjustment {
                        source_id: item.id,
                        attribute_id: modifier.attribute_id.clone(),
                        operation: modifier.operation,
                        value: modifier.value,
                        priority: modifier.priority,
                    }
                }));
            }
            DomainRecord::Condition(condition) if condition.target_id == character.id => {
                let Definition::Condition(condition_definition) =
                    definition(registry, &condition.condition_id)?
                else {
                    return Err(RuntimeError::Unavailable);
                };
                adjustments.extend(condition_definition.modifiers.iter().map(|modifier| {
                    AttributeAdjustment {
                        source_id: condition.id,
                        attribute_id: modifier.attribute_id.clone(),
                        operation: modifier.operation,
                        value: modifier.value,
                        priority: modifier.priority,
                    }
                }));
            }
            _ => {}
        }
    }
    adjustments.sort_by(|left, right| {
        operation_order(left.operation)
            .cmp(&operation_order(right.operation))
            .then_with(|| left.priority.cmp(&right.priority))
            .then_with(|| left.source_id.cmp(&right.source_id))
    });

    let mut views = Vec::with_capacity(character.base_attributes.0.len());
    for (attribute_id, base) in &character.base_attributes.0 {
        let Definition::Attribute(attribute_definition) = definition(registry, attribute_id)?
        else {
            return Err(RuntimeError::Unavailable);
        };
        let mut effective = *base;
        for adjustment in adjustments
            .iter()
            .filter(|adjustment| &adjustment.attribute_id == attribute_id)
        {
            if !attribute_definition
                .allowed_operations
                .contains(&adjustment.operation)
            {
                return Err(RuntimeError::Unavailable);
            }
            effective = match adjustment.operation {
                AttributeOperation::Flat => effective.checked_add(adjustment.value),
                AttributeOperation::Multiply => effective.checked_mul(adjustment.value),
                AttributeOperation::Override => Ok(adjustment.value),
                AttributeOperation::ClampMinimum => Ok(effective.max(adjustment.value)),
                AttributeOperation::ClampMaximum => Ok(effective.min(adjustment.value)),
            }
            .map_err(|error| RuntimeError::World(loreloom_world::WorldError::Fixed(error)))?;
        }
        views.push(AttributeView {
            attribute_id: attribute_id.clone(),
            display_name: attribute_definition.display_name.clone(),
            base: *base,
            effective: effective.clamp(attribute_definition.minimum, attribute_definition.maximum),
        });
    }
    if adjustments.iter().any(|adjustment| {
        !character
            .base_attributes
            .0
            .contains_key(&adjustment.attribute_id)
    }) {
        return Err(RuntimeError::Unavailable);
    }
    Ok(views)
}

const fn operation_order(operation: AttributeOperation) -> u8 {
    match operation {
        AttributeOperation::Flat => 0,
        AttributeOperation::Multiply => 1,
        AttributeOperation::Override => 2,
        AttributeOperation::ClampMinimum => 3,
        AttributeOperation::ClampMaximum => 4,
    }
}

fn parameter_views(
    records: &[DomainRecord],
    session_parameters: &BTreeMap<ContentDefinitionId, ParameterValue>,
    registry: &DefinitionRegistry,
) -> Result<Vec<ParameterSetView>, RuntimeError> {
    let mut sets = records
        .iter()
        .filter_map(|record| match record {
            DomainRecord::ParameterSet(set) => Some(set),
            _ => None,
        })
        .map(|set| {
            let mut values = set
                .values
                .iter()
                .chain(session_parameters.iter().filter(|(parameter_id, _)| {
                    registry
                        .get(parameter_id)
                        .is_some_and(|registered| registered.origin.pack_id == set.schema_id)
                }))
                .filter_map(|(parameter_id, value)| {
                    let registered = registry.get(parameter_id)?;
                    let Definition::Parameter(parameter) = &registered.definition else {
                        return None;
                    };
                    (parameter.visibility == ParameterVisibility::Public).then(|| {
                        ParameterValueView {
                            parameter_id: parameter_id.clone(),
                            display_name: parameter.display_name.clone(),
                            value: value.clone(),
                        }
                    })
                })
                .collect::<Vec<_>>();
            values.sort_by(|left, right| left.parameter_id.cmp(&right.parameter_id));
            Ok(ParameterSetView {
                set_id: set.id,
                schema_id: set.schema_id.clone(),
                values,
            })
        })
        .collect::<Result<Vec<_>, RuntimeError>>()?;
    sets.sort_by_key(|set| set.set_id);
    Ok(sets)
}

fn active_event_views(
    records: &[DomainRecord],
    registry: &DefinitionRegistry,
    actor_id: ActorId,
) -> Result<Vec<ActiveEventView>, RuntimeError> {
    let character = records
        .iter()
        .find_map(|record| match record {
            DomainRecord::Character(character) if character.id == actor_id => Some(character),
            _ => None,
        })
        .ok_or(RuntimeError::Unavailable)?;
    let place = records
        .iter()
        .find_map(|record| match record {
            DomainRecord::Place(place) if place.id == character.location => Some(place),
            _ => None,
        })
        .ok_or(RuntimeError::Unavailable)?;
    let mut views = records
        .iter()
        .filter_map(|record| match record {
            DomainRecord::EventInstance(event)
                if event.status == loreloom_core::EventStatus::Active
                    && event
                        .scene_id
                        .is_none_or(|scene_id| scene_id == place.scene_id) =>
            {
                Some(event)
            }
            _ => None,
        })
        .map(|event| {
            let Definition::Event(event_definition) = definition(registry, &event.definition_id)?
            else {
                return Err(RuntimeError::Unavailable);
            };
            let node = event_definition
                .nodes
                .iter()
                .find(|node| node.id == event.current_node)
                .ok_or(RuntimeError::Unavailable)?;
            let mut options = node
                .options
                .iter()
                .filter(|option| predicates_match(&option.visible_if, character, place, records))
                .map(|option| EventOptionView {
                    option_id: option.id.clone(),
                    display_name: option.display_name.clone(),
                    enabled: predicates_match(&option.enabled_if, character, place, records),
                })
                .collect::<Vec<_>>();
            options.sort_by(|left, right| left.option_id.cmp(&right.option_id));
            Ok(ActiveEventView {
                event_id: event.id,
                definition_id: event.definition_id.clone(),
                display_name: event_definition.display_name.clone(),
                current_node: event.current_node.clone(),
                node_text: node.text.clone(),
                options,
            })
        })
        .collect::<Result<Vec<_>, RuntimeError>>()?;
    views.sort_by_key(|event| event.event_id);
    Ok(views)
}

fn predicates_match(
    predicates: &[PredicateDefinition],
    character: &loreloom_core::CharacterRecord,
    place: &loreloom_core::PlaceRecord,
    records: &[DomainRecord],
) -> bool {
    predicates
        .iter()
        .all(|predicate| predicate_matches(predicate, character, place, records))
}

fn predicate_matches(
    predicate: &PredicateDefinition,
    character: &loreloom_core::CharacterRecord,
    place: &loreloom_core::PlaceRecord,
    records: &[DomainRecord],
) -> bool {
    match predicate {
        PredicateDefinition::ResourceAtLeast {
            resource_id,
            amount,
        } => character
            .resources
            .get(resource_id)
            .is_some_and(|resource| resource.current >= *amount),
        PredicateDefinition::HasCondition { condition_id } => records.iter().any(|record| {
            matches!(record, DomainRecord::Condition(condition)
                    if condition.target_id == character.id
                        && &condition.condition_id == condition_id)
        }),
        PredicateDefinition::HasTag { tag_id } => {
            character.profile.narrative_tags.contains(tag_id) || place.tags.contains(tag_id)
        }
        PredicateDefinition::Not { predicate } => {
            !predicate_matches(predicate, character, place, records)
        }
        PredicateDefinition::All { predicates } => {
            predicates_match(predicates, character, place, records)
        }
        PredicateDefinition::Any { predicates } => predicates
            .iter()
            .any(|predicate| predicate_matches(predicate, character, place, records)),
    }
}

fn transcript_records(records: &[DomainRecord]) -> Vec<loreloom_core::TranscriptItemRecord> {
    let mut transcripts = records
        .iter()
        .filter_map(|record| match record {
            DomainRecord::TranscriptItem(item) => Some(item.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    transcripts.sort_by_key(|item| (item.revision, item.id));
    transcripts
}

fn transcript_window(records: &[DomainRecord], limit: usize) -> TranscriptWindow {
    let mut items = transcript_records(records);
    let start = items.len().saturating_sub(limit);
    let before_cursor = (start > 0).then(|| items[start].id);
    if start > 0 {
        items.drain(..start);
    }
    TranscriptWindow {
        items,
        before_cursor,
    }
}

fn tail<T>(values: impl IntoIterator<Item = T>, limit: usize) -> Vec<T> {
    let mut values = values.into_iter().collect::<Vec<_>>();
    if values.len() > limit {
        values.drain(..values.len() - limit);
    }
    values
}

fn require_revision(inner: &RuntimeWorld, expected: Revision) -> Result<(), RuntimeError> {
    if inner.world.revision() == expected {
        Ok(())
    } else {
        Err(RuntimeError::Unavailable)
    }
}

fn skill_is_available(
    character: &loreloom_core::CharacterRecord,
    grant: &loreloom_core::SkillGrantRecord,
    skill: &loreloom_content::SkillDefinition,
    clock: loreloom_core::WorldTime,
) -> bool {
    grant.enabled
        && skill.kind == loreloom_content::SkillKind::Active
        && grant.ready_at.is_none_or(|ready| ready <= clock)
        && skill.costs.iter().all(|cost| {
            character
                .resources
                .get(&cost.resource_id)
                .is_some_and(|pool| pool.current >= cost.amount)
        })
}

fn npc_draft_schema() -> JsonValue {
    json!({
        "type": "object",
        "required": [
            "display_name", "profile", "agent_profile", "base_attributes", "resources",
            "conditions", "inventory", "skills", "knowledge", "goals"
        ],
        "properties": {
            "display_name": { "type": "string" },
            "profile": {
                "type": "object",
                "required": ["summary", "values", "speaking_style", "narrative_tags"],
                "properties": {
                    "summary": { "type": "string" },
                    "values": { "type": "array", "items": { "type": "string" } },
                    "speaking_style": { "type": "string" },
                    "narrative_tags": {
                        "type": "array",
                        "items": { "type": "string" },
                        "uniqueItems": true
                    }
                },
                "additionalProperties": false
            },
            "agent_profile": {
                "anyOf": [{ "type": "string" }, { "type": "null" }]
            },
            "base_attributes": {
                "type": "object",
                "additionalProperties": { "type": "integer" }
            },
            "resources": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["resource_id", "current", "base_maximum"],
                    "properties": {
                        "resource_id": { "type": "string" },
                        "current": { "type": "integer" },
                        "base_maximum": { "type": "integer" }
                    },
                    "additionalProperties": false
                }
            },
            "conditions": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["condition_id", "stacks", "intensity"],
                    "properties": {
                        "condition_id": { "type": "string" },
                        "stacks": { "type": "integer", "minimum": 1 },
                        "intensity": { "type": "integer" }
                    },
                    "additionalProperties": false
                }
            },
            "inventory": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["local_key", "item_id", "quantity"],
                    "properties": {
                        "local_key": { "type": "string" },
                        "item_id": { "type": "string" },
                        "quantity": { "type": "integer", "minimum": 1 },
                        "parent_local_key": {
                            "anyOf": [{ "type": "string" }, { "type": "null" }]
                        }
                    },
                    "additionalProperties": false
                }
            },
            "skills": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["skill_id", "rank", "proficiency", "enabled"],
                    "properties": {
                        "skill_id": { "type": "string" },
                        "rank": { "type": "integer", "minimum": 1 },
                        "proficiency": { "type": "integer", "minimum": 0 },
                        "enabled": { "type": "boolean" }
                    },
                    "additionalProperties": false
                }
            },
            "knowledge": { "type": "array", "items": { "type": "object" } },
            "goals": { "type": "array", "items": { "type": "object" } }
        },
        "additionalProperties": false
    })
}

fn page_schema() -> JsonValue {
    json!({
        "type": "object",
        "properties": {
            "after": { "type": "string" },
            "limit": {
                "type": "integer",
                "minimum": 1,
                "maximum": TOOL_PAGE_MAXIMUM
            }
        },
        "additionalProperties": false
    })
}

fn parse_page_arguments(arguments: &JsonValue) -> Result<(Option<ObjectId>, usize), RuntimeError> {
    let object = arguments.as_object().ok_or(RuntimeError::InvalidInput)?;
    if object
        .keys()
        .any(|key| !matches!(key.as_str(), "after" | "limit"))
    {
        return Err(RuntimeError::InvalidInput);
    }
    let after = object
        .get("after")
        .map(|value| {
            value
                .as_str()
                .ok_or(RuntimeError::InvalidInput)?
                .parse()
                .map_err(RuntimeError::Identity)
        })
        .transpose()?;
    let limit = match object.get("limit") {
        Some(value) => usize::try_from(value.as_u64().ok_or(RuntimeError::InvalidInput)?)
            .map_err(|_| RuntimeError::InvalidInput)?,
        None => TOOL_PAGE_DEFAULT,
    };
    if !(1..=TOOL_PAGE_MAXIMUM).contains(&limit) {
        return Err(RuntimeError::InvalidInput);
    }
    Ok((after, limit))
}

fn parse_object_id(arguments: &JsonValue, field: &str) -> Result<ObjectId, RuntimeError> {
    arguments
        .get(field)
        .and_then(JsonValue::as_str)
        .ok_or(RuntimeError::InvalidInput)?
        .parse()
        .map_err(RuntimeError::Identity)
}

fn parse_u32(arguments: &JsonValue, field: &str) -> Result<u32, RuntimeError> {
    let value = arguments
        .get(field)
        .and_then(JsonValue::as_u64)
        .ok_or(RuntimeError::InvalidInput)?;
    u32::try_from(value).map_err(|_| RuntimeError::InvalidInput)
}

fn parse_skill_target(arguments: &JsonValue) -> Result<SkillTargetRef, RuntimeError> {
    let target = arguments
        .get("target")
        .cloned()
        .ok_or(RuntimeError::InvalidInput)?;
    serde_json::from_value(target).map_err(|source| RuntimeError::json("skill_target", source))
}

fn parse_definition_id(
    arguments: &JsonValue,
    field: &str,
) -> Result<ContentDefinitionId, RuntimeError> {
    arguments
        .get(field)
        .and_then(JsonValue::as_str)
        .ok_or(RuntimeError::InvalidInput)?
        .parse()
        .map_err(RuntimeError::Identity)
}

fn parse_gameplay_action(
    arguments: &JsonValue,
) -> Result<
    (
        ContentDefinitionId,
        BTreeMap<ContentDefinitionId, ParameterValue>,
    ),
    RuntimeError,
> {
    let action_id = parse_definition_id(arguments, "action_id")?;
    let values = arguments
        .get("arguments")
        .cloned()
        .ok_or(RuntimeError::InvalidInput)?;
    let arguments = serde_json::from_value(values)
        .map_err(|source| RuntimeError::json("tool_arguments", source))?;
    Ok((action_id, arguments))
}

fn committed_json(outcome: CommittedAction) -> JsonValue {
    let event_ids = outcome.event_ids;
    json!({
        "revision": outcome.revision,
        "event_id": event_ids.first(),
        "event_ids": event_ids,
        "summary": outcome.safe_summary,
    })
}

fn tool_result(call: &ToolCall, value: JsonValue, is_error: bool) -> ToolResult {
    ToolResult {
        call_id: call.id.clone(),
        content: vec![ToolResultContent::Json { value }],
        is_error,
    }
}

fn runtime_error_json(error: &RuntimeError) -> JsonValue {
    match error {
        RuntimeError::SceneTransitionTargetUnavailable => json!({
            "code": error.code(),
            "recovery_tool": "list_scene_transitions",
            "retry_unchanged": false,
        }),
        RuntimeError::WorldTopologyConflict => json!({
            "code": error.code(),
            "retry_unchanged": false,
        }),
        _ => json!({ "code": error.code() }),
    }
}
