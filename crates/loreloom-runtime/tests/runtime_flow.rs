use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroU32,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use armillae_core::{
    AssistantContent, CompletionRequest, CompletionResponse, FinishReason, TextContent, ToolCall,
    ToolCallId, ToolResultContent,
};
use armillae_llm::{
    BoxFuture as BridgeFuture, BridgeCapabilities, BridgeError, CompletionStream, LlmBridge,
    MockBridge, MockResponse, ProjectionReport,
};
use armillae_tools::{ToolContext, ToolExecutor};
use loreloom_agent::{
    AgentDefinition, AgentToolContext, NarratorPlan, NpcModelOutput, NpcTurnRequest, NpcTurnStatus,
};
use loreloom_content::{
    AgentProfileDefinition, AttributeDefinition, CONTENT_SCHEMA_V1, ContainerDefinition,
    ContentDocument, ContentPackContext, Definition, DefinitionRegistry, EffectDefinition,
    EventDefinition, EventNodeDefinition, EventOptionDefinition, GameplayActionDefinition,
    ItemDefinition, ParameterDefinition, ParameterPersistence, ParameterType, ParameterVisibility,
    PlaceDefinition, SceneDefinition, parse_content_hash,
};
use loreloom_core::{
    ActionId, ActionState, ActorId, AgentBinding, AttributeAdjustment, AttributeOperation,
    BaseAttributes, CharacterController, CharacterLifetime, CharacterProfile, CharacterRecord,
    ContentDefinitionId, ContentOrigin, DisplayName, DomainRecord, EntityOrigin,
    EventInstanceRecord, EventStatus, Fixed, LifeState, LockedMod, LongText, ModId, ModLock,
    ModSourceKind, ObjectId, ParameterSetRecord, ParameterValue, PlaceRecord, Posture, Revision,
    SAVE_FORMAT_V1, SaveId, SaveManifest, SceneRecord, SessionId, ShortText, StackState,
    SystemIdGenerator, TranscriptSpeaker, WorldCommand, WorldCommandKind, WorldId,
    WorldStateRecord, WorldTime,
};
use loreloom_runtime::{
    GameRuntime, RuntimeConfig, RuntimeError, RuntimeToolExecutor, WorldService,
};
use loreloom_store::{CommitRequest, CommitResult, SaveStore};
use loreloom_world::{GameWorld, WorldConfig};
use semver::Version;
use serde_json::{Value as JsonValue, json};
use tempfile::TempDir;

fn parse<T: std::str::FromStr>(value: &str) -> T
where
    T::Err: std::fmt::Debug,
{
    value.parse().expect("fixture identifier")
}

fn definition_id(kind: &str, key: &str) -> ContentDefinitionId {
    format!("games.loreloom.runtime:{kind}/{key}")
        .parse()
        .expect("definition id")
}

fn object_id(suffix: &str) -> ObjectId {
    format!("obj_01890f6a-{suffix}-7d4e-8f90-123456789abc")
        .parse()
        .expect("object id")
}

fn name(value: &str) -> DisplayName {
    DisplayName::new(value).expect("display name")
}

fn text(value: &str) -> ShortText {
    ShortText::new(value).expect("short text")
}

struct Fixture {
    registry: DefinitionRegistry,
    records: Vec<DomainRecord>,
    manifest: SaveManifest,
    world_config: WorldConfig,
    player: ActorId,
    npc: ActorId,
    scene: ObjectId,
    profile_id: ContentDefinitionId,
}

fn fixture() -> Fixture {
    let mod_id = ModId::parse("games.loreloom.runtime").expect("mod id");
    let pack_id = definition_id("pack", "runtime");
    let profile_id = definition_id("agent_profile", "keeper");
    let attribute_id = definition_id("attribute", "resolve");
    let public_parameter = definition_id("parameter", "rain_count");
    let hidden_parameter = definition_id("parameter", "secret_count");
    let session_parameter = definition_id("parameter", "hint_seen");
    let gameplay_action = definition_id("gameplay_action", "mark_rain");
    let event_definition = definition_id("event", "rain");
    let event_node = definition_id("event_node", "rain_entry");
    let event_option = definition_id("event_option", "listen");
    let inventory_definition = definition_id("item", "inventory");
    let place_definition = definition_id("place", "hall");
    let scene_definition = definition_id("scene", "inn");
    let registry = DefinitionRegistry::build(
        ContentPackContext {
            mod_id,
            mod_version: Version::new(1, 0, 0),
            pack_id: pack_id.clone(),
            content_version: 1,
            content_hash: parse_content_hash("b".repeat(64)).expect("content hash"),
        },
        [ContentDocument {
            schema_version: CONTENT_SCHEMA_V1,
            definitions: vec![
                Definition::AgentProfile(AgentProfileDefinition {
                    id: profile_id.clone(),
                    display_name: name("Keeper"),
                    system_style: text("Speak tersely and guard the inn."),
                    model_alias: text("mock-npc"),
                    tool_capabilities: BTreeSet::from([text("advance_time")]),
                    autonomy: loreloom_core::AutonomyMode::Directed,
                }),
                Definition::Attribute(AttributeDefinition {
                    id: attribute_id.clone(),
                    display_name: name("Resolve"),
                    minimum: Fixed::ZERO,
                    maximum: Fixed::from_integer(20).expect("attribute maximum"),
                    allowed_operations: BTreeSet::from([AttributeOperation::Flat]),
                }),
                Definition::Parameter(ParameterDefinition {
                    id: public_parameter.clone(),
                    display_name: name("Rain count"),
                    value_type: ParameterType::Counter {
                        minimum: 0,
                        maximum: 100,
                    },
                    default: ParameterValue::Counter(0),
                    visibility: ParameterVisibility::Public,
                    persistence: ParameterPersistence::Save,
                }),
                Definition::Parameter(ParameterDefinition {
                    id: session_parameter.clone(),
                    display_name: name("Hint seen"),
                    value_type: ParameterType::Bool,
                    default: ParameterValue::Bool(false),
                    visibility: ParameterVisibility::Public,
                    persistence: ParameterPersistence::Session,
                }),
                Definition::GameplayAction(GameplayActionDefinition {
                    id: gameplay_action,
                    display_name: name("Mark the rain"),
                    capability: text("gameplay.weather"),
                    parameters: Vec::new(),
                    predicates: Vec::new(),
                    effects: vec![
                        EffectDefinition::SetParameter {
                            parameter_id: public_parameter.clone(),
                            value: ParameterValue::Counter(4),
                        },
                        EffectDefinition::SetParameter {
                            parameter_id: session_parameter,
                            value: ParameterValue::Bool(true),
                        },
                    ],
                }),
                Definition::Parameter(ParameterDefinition {
                    id: hidden_parameter.clone(),
                    display_name: name("Secret count"),
                    value_type: ParameterType::Counter {
                        minimum: 0,
                        maximum: 100,
                    },
                    default: ParameterValue::Counter(0),
                    visibility: ParameterVisibility::Hidden,
                    persistence: ParameterPersistence::Save,
                }),
                Definition::Event(EventDefinition {
                    id: event_definition.clone(),
                    display_name: name("Rain at the Inn"),
                    entry_node: event_node.clone(),
                    nodes: vec![EventNodeDefinition {
                        id: event_node.clone(),
                        text: text("The rain asks for attention."),
                        options: vec![EventOptionDefinition {
                            id: event_option,
                            display_name: name("Listen"),
                            visible_if: Vec::new(),
                            enabled_if: Vec::new(),
                            effects: Vec::new(),
                            next_node: None,
                        }],
                    }],
                }),
                Definition::Item(ItemDefinition {
                    id: inventory_definition.clone(),
                    display_name: name("Inventory"),
                    description: text("A private inventory root."),
                    tags: BTreeSet::new(),
                    stack_limit: NonZeroU32::MIN,
                    unit_weight_grams: Fixed::ZERO,
                    durability: None,
                    container: Some(ContainerDefinition {
                        max_weight_grams: Fixed::from_integer(1_000).expect("weight"),
                        max_children: 16,
                    }),
                    equipment_slots: BTreeSet::new(),
                    modifiers: Vec::new(),
                }),
                Definition::Place(PlaceDefinition {
                    id: place_definition.clone(),
                    display_name: name("Hall"),
                    description: text("A quiet timber hall."),
                    tags: BTreeSet::new(),
                    edges: BTreeSet::new(),
                }),
                Definition::Scene(SceneDefinition {
                    id: scene_definition.clone(),
                    display_name: name("Old Inn"),
                    framing: text("Rain taps on the shutters."),
                    entry_place: place_definition.clone(),
                    places: BTreeSet::from([place_definition.clone()]),
                    characters: Vec::new(),
                }),
            ],
        }],
    )
    .expect("registry");

    let player = ActorId::from(object_id("2b3c"));
    let npc = ActorId::from(object_id("2b3d"));
    let scene = object_id("2b3e");
    let place = object_id("2b3f");
    let player_root = object_id("2b40");
    let npc_root = object_id("2b41");
    let origin = |id: &ContentDefinitionId| -> ContentOrigin {
        registry.get(id).expect("definition origin").origin.clone()
    };
    let profile = |summary: &str| CharacterProfile {
        summary: text(summary),
        values: Vec::new(),
        speaking_style: text("Direct."),
        narrative_tags: BTreeSet::new(),
    };
    let mut records = vec![
        DomainRecord::WorldState(WorldStateRecord {
            id: parse::<WorldId>("wld_01890f6a-2b42-7d4e-8f90-123456789abc"),
            player_actor: player,
            active_scene: scene,
            clock: WorldTime::ZERO,
            rng_seed: [9; 32],
        }),
        DomainRecord::Scene(SceneRecord {
            id: scene,
            display_name: name("Old Inn"),
            framing: text("Rain taps on the shutters."),
            entry_place: place,
            active: true,
            origin: origin(&scene_definition),
        }),
        DomainRecord::Place(PlaceRecord {
            id: place,
            scene_id: scene,
            display_name: name("Hall"),
            description: text("A quiet timber hall."),
            tags: BTreeSet::new(),
            origin: origin(&place_definition),
        }),
        character(
            player,
            "Traveler",
            CharacterController::Player,
            player_root,
            place,
            profile("A road-worn traveler."),
            None,
        ),
        character(
            npc,
            "Mira",
            CharacterController::Agent,
            npc_root,
            place,
            profile("The innkeeper."),
            Some(AgentBinding {
                profile_id: profile_id.clone(),
                enabled: true,
                autonomy: loreloom_core::AutonomyMode::Directed,
            }),
        ),
        inventory(
            player_root,
            player,
            place,
            &inventory_definition,
            origin(&inventory_definition),
        ),
        inventory(
            npc_root,
            npc,
            place,
            &inventory_definition,
            origin(&inventory_definition),
        ),
        DomainRecord::ParameterSet(ParameterSetRecord {
            id: object_id("2b45"),
            schema_id: pack_id,
            values: BTreeMap::from([
                (public_parameter, ParameterValue::Counter(3)),
                (hidden_parameter, ParameterValue::Counter(9)),
            ]),
        }),
        DomainRecord::EventInstance(EventInstanceRecord {
            id: object_id("2b46"),
            definition_id: event_definition,
            current_node: event_node,
            scene_id: Some(scene),
            started_at: WorldTime::ZERO,
            status: EventStatus::Active,
            committed_options: Vec::new(),
        }),
    ];
    let DomainRecord::Character(player_record) = &mut records[3] else {
        unreachable!("player fixture record")
    };
    player_record.base_attributes = BaseAttributes(BTreeMap::from([(
        attribute_id.clone(),
        Fixed::from_integer(10).expect("base attribute"),
    )]));
    player_record
        .attribute_adjustments
        .push(AttributeAdjustment {
            source_id: object_id("2b44"),
            attribute_id,
            operation: AttributeOperation::Flat,
            value: Fixed::from_integer(2).expect("attribute adjustment"),
            priority: 0,
        });
    Fixture {
        registry,
        records,
        manifest: SaveManifest {
            format_version: SAVE_FORMAT_V1,
            save_id: parse::<SaveId>("sav_01890f6a-2b43-7d4e-8f90-123456789abc"),
            world_id: parse::<WorldId>("wld_01890f6a-2b42-7d4e-8f90-123456789abc"),
            mod_lock: ModLock::default(),
        },
        world_config: WorldConfig {
            inventory_root_definition: inventory_definition,
            spawn_system_definition: definition_id("system", "spawn"),
            rule_limits: Default::default(),
        },
        player,
        npc,
        scene,
        profile_id,
    }
}

fn character(
    id: ActorId,
    display_name: &str,
    controller: CharacterController,
    inventory_root: ObjectId,
    location: ObjectId,
    profile: CharacterProfile,
    agent_binding: Option<AgentBinding>,
) -> DomainRecord {
    DomainRecord::Character(CharacterRecord {
        id,
        display_name: name(display_name),
        profile,
        controller,
        lifetime: CharacterLifetime::Persistent,
        location,
        inventory_root,
        agent_binding,
        base_attributes: BaseAttributes::default(),
        attribute_adjustments: Vec::new(),
        resources: Default::default(),
        life_state: LifeState::Alive,
        action_state: ActionState::Idle,
        posture: Posture::Standing,
        origin: EntityOrigin::System {
            source: definition_id("system", "bootstrap"),
        },
    })
}

fn inventory(
    id: ObjectId,
    owner: ActorId,
    place: ObjectId,
    definition_id: &ContentDefinitionId,
    origin: ContentOrigin,
) -> DomainRecord {
    DomainRecord::Item(loreloom_core::ItemRecord {
        id,
        definition_id: definition_id.clone(),
        stack: StackState(NonZeroU32::MIN),
        durability: None,
        container: Some(loreloom_core::ContainerState {
            max_weight_grams: Fixed::from_integer(1_000).expect("weight"),
            max_children: 16,
        }),
        contained_by: None,
        owned_by: Some(owner),
        equipped: None,
        located_at: Some(place),
        custom_name: None,
        bound_actor: Some(owner),
        parameters: Default::default(),
        instance_adjustments: Vec::new(),
        origin: EntityOrigin::Content { origin },
    })
}

fn text_response(text: impl Into<String>) -> CompletionResponse {
    CompletionResponse {
        id: None,
        model: Some("mock-narrator".to_owned()),
        content: vec![AssistantContent::Text(TextContent::new(text))],
        finish_reason: Some(FinishReason::Stop),
        usage: None,
        provider_metadata: JsonValue::Null,
    }
}

enum SupportMode {
    Committed,
    Empty,
    Fabricated,
}

struct NarratorBridge {
    plan: NarratorPlan,
    support: SupportMode,
    calls: AtomicUsize,
}

impl NarratorBridge {
    fn new(plan: NarratorPlan, support: SupportMode) -> Self {
        Self {
            plan,
            support,
            calls: AtomicUsize::new(0),
        }
    }
}

impl LlmBridge for NarratorBridge {
    fn capabilities(&self) -> BridgeCapabilities {
        BridgeCapabilities::all()
    }

    fn project(&self, _request: &CompletionRequest) -> Result<ProjectionReport, BridgeError> {
        Ok(ProjectionReport::exact("runtime-test"))
    }

    fn complete<'a>(
        &'a self,
        request: CompletionRequest,
    ) -> BridgeFuture<'a, Result<CompletionResponse, BridgeError>> {
        Box::pin(async move {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            if call == 0 {
                return Ok(text_response(
                    serde_json::to_string(&self.plan).expect("plan serialization"),
                ));
            }
            let payload = request
                .messages
                .iter()
                .rev()
                .find_map(|message| {
                    message.content.iter().find_map(|part| match part {
                        armillae_core::ContentPart::Text(text) => {
                            serde_json::from_str::<JsonValue>(&text.text).ok()
                        }
                        _ => None,
                    })
                })
                .ok_or_else(|| BridgeError::InvalidRequest {
                    message: "missing synthesis payload".to_owned(),
                })?;
            let revision = payload["payload"]["revision"].clone();
            let supporting_events = match self.support {
                SupportMode::Committed => payload["payload"]["committed_events"]
                    .as_array()
                    .and_then(|events| events.last())
                    .map(|event| vec![event["id"].clone()])
                    .unwrap_or_default(),
                SupportMode::Empty => Vec::new(),
                SupportMode::Fabricated => {
                    vec![json!("evt_01890f6a-2b70-7d4e-8f90-123456789abc")]
                }
            };
            Ok(text_response(
                serde_json::to_string(&json!({
                    "kind": "final",
                    "based_on_revision": revision,
                    "narration": "The inn settles into a deliberate silence.",
                    "supporting_events": supporting_events
                }))
                .expect("synthesis serialization"),
            ))
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

fn request(actor_id: ActorId, scene_id: ObjectId) -> NpcTurnRequest {
    NpcTurnRequest {
        request_id: parse("ntr_01890f6a-2b60-7d4e-8f90-123456789abc"),
        actor_id,
        scene_id,
        based_on_revision: Revision::new(1),
        assignment: loreloom_agent::AssignmentText::new("Listen, then let one second pass.")
            .expect("assignment"),
    }
}

fn definition(profile_id: ContentDefinitionId) -> AgentDefinition {
    AgentDefinition {
        profile_id,
        system_style: LongText::new("Be concise.").expect("system style"),
        model_alias: text("mock-npc"),
        allowed_tools: BTreeSet::from(["advance_time".to_owned()]),
    }
}

fn npc_bridge() -> Arc<MockBridge> {
    let call = ToolCall {
        id: ToolCallId::new("npc-advance").expect("call id"),
        name: "advance_time".to_owned(),
        arguments: json!({ "ticks": 1 }),
    };
    Arc::new(MockBridge::scripted([
        MockResponse::tool_call(call.id, call.name, call.arguments),
        MockResponse::text(
            serde_json::to_string(&NpcModelOutput {
                utterance: Some(
                    loreloom_agent::UtteranceText::new("One moment passes.").expect("utterance"),
                ),
                intent: None,
                claimed_action_description: Some(
                    loreloom_agent::ClaimedActionText::new("waited by the hearth").expect("claim"),
                ),
            })
            .expect("npc output"),
        ),
    ]))
}

#[tokio::test]
async fn world_service_rejects_a_candidate_mod_lock_before_world_materialization() {
    let directory = TempDir::new().expect("temporary save parent");
    let fixture = fixture();
    let store = SaveStore::create(
        directory.path().join("save"),
        fixture.manifest,
        fixture.records,
    )
    .await
    .expect("create save");
    let candidate = ModLock {
        mods: vec![LockedMod {
            mod_id: ModId::parse("games.loreloom.other").expect("candidate Mod ID"),
            version: Version::new(1, 0, 0),
            content_hash: parse_content_hash("e".repeat(64)).expect("candidate hash"),
            manifest_schema: 1,
            content_schema: CONTENT_SCHEMA_V1,
            source_kind: ModSourceKind::Builtin,
            dependencies: Vec::new(),
            applied_patches: Vec::new(),
        }],
    };

    assert!(matches!(
        WorldService::open(store, fixture.registry, &candidate, fixture.world_config).await,
        Err(RuntimeError::ContentLockMismatch)
    ));
}

#[tokio::test]
async fn player_narrator_npc_and_surreal_store_form_a_durable_vertical_slice() {
    let directory = TempDir::new().expect("temporary save parent");
    let fixture = fixture();
    let store = SaveStore::create(
        directory.path().join("save"),
        fixture.manifest.clone(),
        fixture.records.clone(),
    )
    .await
    .expect("create save");
    let mut observer = store.connect().await.expect("observer connection");
    let service = WorldService::open(
        store,
        fixture.registry.clone(),
        &fixture.manifest.mod_lock,
        fixture.world_config.clone(),
    )
    .await
    .expect("open world service");
    let plan = NarratorPlan {
        based_on_revision: Revision::new(1),
        npc_turns: vec![request(fixture.npc, fixture.scene)],
    };
    let narrator = Arc::new(NarratorBridge::new(plan, SupportMode::Committed));
    let npc = npc_bridge();
    let mut runtime = GameRuntime::new(
        Arc::clone(&service),
        narrator,
        parse::<SessionId>("ses_01890f6a-2b61-7d4e-8f90-123456789abc"),
        RuntimeConfig::default(),
    );
    runtime.register_npc(
        fixture.npc,
        definition(fixture.profile_id.clone()),
        npc.clone(),
    );

    let outcome = runtime
        .handle_player_input("Ask Mira to listen to the rain.")
        .await
        .expect("complete player turn");

    assert_eq!(outcome.snapshot.revision, Revision::new(3));
    assert_eq!(outcome.snapshot.transcript.items.len(), 2);
    assert_eq!(outcome.snapshot.player.attributes.len(), 1);
    assert_eq!(
        outcome.snapshot.player.attributes[0].display_name.as_str(),
        "Resolve"
    );
    assert_eq!(
        outcome.snapshot.player.attributes[0].effective,
        Fixed::from_integer(12).expect("effective attribute")
    );
    assert_eq!(outcome.snapshot.parameters.len(), 1);
    assert_eq!(outcome.snapshot.parameters[0].values.len(), 2);
    assert!(
        outcome.snapshot.parameters[0]
            .values
            .iter()
            .any(|value| value.display_name.as_str() == "Rain count")
    );
    assert_eq!(outcome.snapshot.active_events.len(), 1);
    assert_eq!(outcome.snapshot.active_events[0].options.len(), 1);
    assert!(outcome.snapshot.active_events[0].options[0].enabled);
    assert_eq!(outcome.npc_results.len(), 1);
    assert_eq!(outcome.npc_results[0].status, NpcTurnStatus::Completed);
    assert_eq!(
        outcome.npc_results[0].observed_revision,
        Some(Revision::new(1))
    );
    assert_eq!(outcome.npc_results[0].final_revision, Revision::new(2));
    assert_eq!(outcome.npc_results[0].world_events.len(), 1);
    assert_eq!(
        outcome.snapshot.supporting_events,
        outcome.npc_results[0].world_events
    );
    assert_eq!(npc.requests().expect("npc requests").len(), 2);

    let loaded = observer.load().await.expect("load durable result");
    assert_eq!(loaded.revision, Revision::new(3));
    assert_eq!(loaded.transcripts.len(), 2);
    let rebuilt = GameWorld::from_records(
        loaded.revision,
        loaded.records,
        fixture.world_config,
        &fixture.registry,
    )
    .expect("rebuild without a provider");
    assert_eq!(rebuilt.world_state().clock, WorldTime::from_ticks(1));
    drop((runtime, service, observer));
}

#[tokio::test]
async fn stale_npc_requests_are_correlated_without_calling_the_provider() {
    let directory = TempDir::new().expect("temporary save parent");
    let fixture = fixture();
    let candidate_mod_lock = fixture.manifest.mod_lock.clone();
    let store = SaveStore::create(
        directory.path().join("save"),
        fixture.manifest,
        fixture.records,
    )
    .await
    .expect("create save");
    let service = WorldService::open(
        store,
        fixture.registry,
        &candidate_mod_lock,
        fixture.world_config,
    )
    .await
    .expect("open service");
    let plan = NarratorPlan {
        based_on_revision: Revision::new(1),
        npc_turns: vec![request(fixture.npc, object_id("2b6f"))],
    };
    let npc = npc_bridge();
    let mut runtime = GameRuntime::new(
        service,
        Arc::new(NarratorBridge::new(plan, SupportMode::Empty)),
        parse("ses_01890f6a-2b62-7d4e-8f90-123456789abc"),
        RuntimeConfig::default(),
    );
    runtime.register_npc(fixture.npc, definition(fixture.profile_id), npc.clone());

    let outcome = runtime
        .handle_player_input("Call for Mira outside this scene.")
        .await
        .expect("complete with stale result");

    assert_eq!(outcome.npc_results.len(), 1);
    assert_eq!(outcome.npc_results[0].status, NpcTurnStatus::Stale);
    assert_eq!(outcome.npc_results[0].observed_revision, None);
    assert!(npc.requests().expect("npc request log").is_empty());
}

#[tokio::test]
async fn synthesis_cannot_cite_an_uncommitted_world_event() {
    let directory = TempDir::new().expect("temporary save parent");
    let fixture = fixture();
    let candidate_mod_lock = fixture.manifest.mod_lock.clone();
    let store = SaveStore::create(
        directory.path().join("save"),
        fixture.manifest,
        fixture.records,
    )
    .await
    .expect("create save");
    let mut observer = store.connect().await.expect("observer connection");
    let service = WorldService::open(
        store,
        fixture.registry,
        &candidate_mod_lock,
        fixture.world_config,
    )
    .await
    .expect("open service");
    let plan = NarratorPlan {
        based_on_revision: Revision::new(1),
        npc_turns: Vec::new(),
    };
    let mut runtime = GameRuntime::new(
        service,
        Arc::new(NarratorBridge::new(plan, SupportMode::Fabricated)),
        parse("ses_01890f6a-2b63-7d4e-8f90-123456789abc"),
        RuntimeConfig::default(),
    );

    let error = runtime
        .handle_player_input("Say that an impossible event occurred.")
        .await
        .expect_err("fabricated event is rejected");

    assert!(matches!(
        error,
        RuntimeError::ModelProtocol {
            stage: "uncommitted_supporting_event"
        }
    ));
    let loaded = observer.load().await.expect("load after rejection");
    assert_eq!(loaded.revision, Revision::new(1));
    assert_eq!(loaded.transcripts.len(), 1);
    assert!(matches!(
        loaded.transcripts[0].speaker,
        TranscriptSpeaker::Player { .. }
    ));
}

#[tokio::test]
async fn external_revision_conflict_recovers_the_candidate_world() {
    let directory = TempDir::new().expect("temporary save parent");
    let fixture = fixture();
    let candidate_mod_lock = fixture.manifest.mod_lock.clone();
    let store = SaveStore::create(
        directory.path().join("save"),
        fixture.manifest,
        fixture.records.clone(),
    )
    .await
    .expect("create save");
    let mut external = store.connect().await.expect("external connection");
    let service = WorldService::open(
        store,
        fixture.registry.clone(),
        &candidate_mod_lock,
        fixture.world_config.clone(),
    )
    .await
    .expect("open service");

    let mut external_world = GameWorld::from_records(
        Revision::ZERO,
        fixture.records,
        fixture.world_config,
        &fixture.registry,
    )
    .expect("external candidate");
    let command = WorldCommand {
        action_id: parse::<ActionId>("act_01890f6a-2b80-7d4e-8f90-123456789abc"),
        actor_id: fixture.player,
        expected_revision: Revision::ZERO,
        kind: WorldCommandKind::AdvanceTime { ticks: 5 },
    };
    let changes = external_world
        .execute(command.clone(), &fixture.registry, &mut SystemIdGenerator)
        .expect("external execution");
    let request = CommitRequest::from_execution(command, changes).expect("external request");
    assert!(matches!(
        external.commit(&request).await.expect("external commit"),
        CommitResult::Committed(_)
    ));

    let executor = RuntimeToolExecutor::new(Arc::clone(&service));
    let result = executor
        .execute(
            ToolContext::new().with_extension(AgentToolContext {
                actor_id: fixture.player,
                revision: Revision::ZERO,
                session_id: parse("ses_01890f6a-2b81-7d4e-8f90-123456789abc"),
                capabilities: Default::default(),
            }),
            ToolCall {
                id: ToolCallId::new("conflicting-call").expect("call id"),
                name: "advance_time".to_owned(),
                arguments: json!({ "ticks": 1 }),
            },
        )
        .await
        .expect("correlated tool result");

    assert!(result.is_error);
    assert_eq!(service.revision().await, Revision::new(1));
    let snapshot = service
        .snapshot(
            parse("ses_01890f6a-2b81-7d4e-8f90-123456789abc"),
            loreloom_core::RuntimePhase::Idle,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .await
        .expect("recovered snapshot");
    assert_eq!(snapshot.scene.clock, WorldTime::from_ticks(5));
}

#[tokio::test]
async fn generic_gameplay_tools_enforce_capabilities_and_preserve_session_overlay_on_recovery() {
    let directory = TempDir::new().expect("temporary save parent");
    let fixture = fixture();
    let store = SaveStore::create(
        directory.path().join("save"),
        fixture.manifest.clone(),
        fixture.records.clone(),
    )
    .await
    .expect("create save");
    let service = WorldService::open(
        store,
        fixture.registry.clone(),
        &fixture.manifest.mod_lock,
        fixture.world_config.clone(),
    )
    .await
    .expect("open service");
    let executor = RuntimeToolExecutor::new(Arc::clone(&service));
    let session_id = parse("ses_01890f6a-2b91-7d4e-8f90-123456789abc");
    let authorized = BTreeSet::from(["gameplay.weather".to_owned()]);

    let listed = executor
        .execute(
            ToolContext::new().with_extension(AgentToolContext {
                actor_id: fixture.player,
                revision: Revision::ZERO,
                session_id,
                capabilities: authorized.clone(),
            }),
            ToolCall {
                id: ToolCallId::new("list-actions").expect("call id"),
                name: "list_gameplay_actions".to_owned(),
                arguments: json!({}),
            },
        )
        .await
        .expect("list result");
    let ToolResultContent::Json { value: listed } = &listed.content[0] else {
        panic!("JSON tool result")
    };
    assert_eq!(listed["actions"].as_array().expect("actions").len(), 1);
    assert_eq!(
        listed["actions"][0]["action_id"],
        json!(definition_id("gameplay_action", "mark_rain"))
    );

    let performed = executor
        .execute(
            ToolContext::new().with_extension(AgentToolContext {
                actor_id: fixture.player,
                revision: Revision::ZERO,
                session_id,
                capabilities: authorized.clone(),
            }),
            ToolCall {
                id: ToolCallId::new("perform-action").expect("call id"),
                name: "perform_gameplay_action".to_owned(),
                arguments: json!({
                    "action_id": definition_id("gameplay_action", "mark_rain"),
                    "arguments": {}
                }),
            },
        )
        .await
        .expect("perform result");
    assert!(!performed.is_error);
    assert_eq!(service.revision().await, Revision::new(1));

    let stale = executor
        .execute(
            ToolContext::new().with_extension(AgentToolContext {
                actor_id: fixture.player,
                revision: Revision::ZERO,
                session_id,
                capabilities: authorized.clone(),
            }),
            ToolCall {
                id: ToolCallId::new("stale-after-session").expect("call id"),
                name: "advance_time".to_owned(),
                arguments: json!({ "ticks": 1 }),
            },
        )
        .await
        .expect("stale result");
    assert!(stale.is_error);
    assert_eq!(service.revision().await, Revision::new(1));

    let snapshot = service
        .snapshot(
            session_id,
            loreloom_core::RuntimePhase::Idle,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .await
        .expect("snapshot");
    let values = &snapshot.parameters[0].values;
    assert!(values.iter().any(|value| {
        value.parameter_id == definition_id("parameter", "rain_count")
            && value.value == ParameterValue::Counter(4)
    }));
    assert!(values.iter().any(|value| {
        value.parameter_id == definition_id("parameter", "hint_seen")
            && value.value == ParameterValue::Bool(true)
    }));

    let denied = executor
        .execute(
            ToolContext::new().with_extension(AgentToolContext {
                actor_id: fixture.player,
                revision: Revision::new(1),
                session_id,
                capabilities: BTreeSet::new(),
            }),
            ToolCall {
                id: ToolCallId::new("denied-action").expect("call id"),
                name: "perform_gameplay_action".to_owned(),
                arguments: json!({
                    "action_id": definition_id("gameplay_action", "mark_rain"),
                    "arguments": {}
                }),
            },
        )
        .await
        .expect("denied result");
    assert!(denied.is_error);
    let ToolResultContent::Json { value: denied } = &denied.content[0] else {
        panic!("JSON tool result")
    };
    assert_eq!(denied["code"], "capability_denied");

    let chosen = executor
        .execute(
            ToolContext::new().with_extension(AgentToolContext {
                actor_id: fixture.player,
                revision: Revision::new(1),
                session_id,
                capabilities: authorized,
            }),
            ToolCall {
                id: ToolCallId::new("choose-option").expect("call id"),
                name: "choose_event_option".to_owned(),
                arguments: json!({
                    "event_instance_id": object_id("2b46"),
                    "option_id": definition_id("event_option", "listen")
                }),
            },
        )
        .await
        .expect("choose result");
    assert!(!chosen.is_error);
    assert_eq!(service.revision().await, Revision::new(2));
}
