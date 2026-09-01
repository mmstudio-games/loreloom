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
    ToolCallId, ToolResult, ToolResultContent,
};
use armillae_llm::{
    BoxFuture as BridgeFuture, BridgeCapabilities, BridgeError, CompletionStream, ErrorMetadata,
    LlmBridge, MockBridge, MockResponse, ProjectionReport,
};
use armillae_tools::{ToolContext, ToolExecutor};
use loreloom_agent::{
    AgentDefinition, AgentRunner, AgentToolContext, CancellationToken, CreateNpcRequest,
    ModelFailureCategory, ModelFailureStage, ModelInvocationKind, NarratorDefinition, NarratorPlan,
    NpcCreationMode, NpcCreationSource, NpcLifetime, NpcTurnRequest, NpcTurnStatus, ResourceBudget,
    TurnInvocation, TurnStatus,
};
use loreloom_content::{
    AgentProfileDefinition, AttributeDefinition, CONTENT_SCHEMA_V1, CharacterDefinition,
    ConditionDefinition, ContainerDefinition, ContentDocument, ContentPackContext, Definition,
    DefinitionRegistry, DurationPolicy, EffectDefinition, EquipmentSlotDefinition, EventDefinition,
    EventNodeDefinition, EventOptionDefinition, GameplayActionDefinition, GenerationPolicy,
    InitialCharacterController, InitialCharacterLifetime, ItemDefinition, NpcDraft,
    ParameterDefinition, ParameterPersistence, ParameterType, ParameterVisibility, PlaceDefinition,
    ResourceCost, ResourceDefinition, ResourceMaximumPolicy, SceneCharacterDefinition,
    SceneDefinition, SkillDefinition, SkillKind, SkillTarget, StackPolicy, SymptomDefinition,
    TagDefinition, parse_content_hash,
};
use loreloom_core::{
    ActionId, ActionState, ActorId, AgentBinding, AttributeAdjustment, AttributeOperation,
    BaseAttributes, CharacterController, CharacterLifetime, CharacterProfile, CharacterRecord,
    ConditionRecord, ConditionSource, ContentDefinitionId, ContentOrigin,
    DIAGNOSED_CONDITION_PREDICATE_ID, DisplayName, DomainRecord, EntityOrigin, EventInstanceRecord,
    EventStatus, FactSource, FactSubject, FactValue, Fixed, GenerationSource, GoalRecord,
    GoalSource, GoalStatus, IntensityPolicy, ItemRecord, KnowledgeStatus, KnownFactRecord,
    LifeState, LockedMod, LongText, ModId, ModLock, ModSourceKind, ObjectId, ParameterSetRecord,
    ParameterValue, PlaceRecord, Posture, ResourcePool, Revision, SAVE_FORMAT_V1, SaveId,
    SaveManifest, SceneRecord, SessionId, ShortText, SkillGrantRecord, SkillSource,
    SpawnConstraints, StackState, SystemIdGenerator, TranscriptSpeaker, WorldCommand,
    WorldCommandKind, WorldEventKind, WorldId, WorldLock, WorldStateRecord, WorldTime,
};
use loreloom_runtime::{
    ContextProjectionPolicy, GameRuntime, NpcResourcePolicy, OrchestrationBudget, RuntimeConfig,
    RuntimeError, RuntimeToolExecutor, WorldService,
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

fn tool_result_json(result: &ToolResult) -> &JsonValue {
    match result.content.as_slice() {
        [ToolResultContent::Json { value }] => value,
        content => panic!("expected one JSON ToolResult, got {content:?}"),
    }
}

struct Fixture {
    registry: DefinitionRegistry,
    records: Vec<DomainRecord>,
    manifest: SaveManifest,
    world_config: WorldConfig,
    player: ActorId,
    npc: ActorId,
    scene: ObjectId,
    place: ObjectId,
    profile_id: ContentDefinitionId,
    preset_id: ContentDefinitionId,
    condition_id: ContentDefinitionId,
    forest_scene_definition: ContentDefinitionId,
}

fn fixture() -> Fixture {
    let mod_id = ModId::parse("games.loreloom.runtime").expect("mod id");
    let pack_id = definition_id("pack", "runtime");
    let profile_id = definition_id("agent_profile", "keeper");
    let preset_id = definition_id("character", "watcher");
    let attribute_id = definition_id("attribute", "resolve");
    let public_parameter = definition_id("parameter", "rain_count");
    let hidden_parameter = definition_id("parameter", "secret_count");
    let session_parameter = definition_id("parameter", "hint_seen");
    let gameplay_action = definition_id("gameplay_action", "mark_rain");
    let event_definition = definition_id("event", "rain");
    let event_node = definition_id("event_node", "rain_entry");
    let event_option = definition_id("event_option", "listen");
    let inventory_definition = definition_id("item", "inventory");
    let stack_definition = definition_id("item", "token_stack");
    let gear_definition = definition_id("item", "hand_lantern");
    let equipment_slot = definition_id("equipment_slot", "hand");
    let resource_definition = definition_id("resource", "focus");
    let skill_definition = definition_id("skill", "steady_breath");
    let condition_id = definition_id("condition", "shivering");
    let place_definition = definition_id("place", "hall");
    let scene_definition = definition_id("scene", "inn");
    let forest_place_definition = definition_id("place", "forest_path");
    let forest_scene_definition = definition_id("scene", "forest");
    let diagnosis_predicate = ContentDefinitionId::parse(DIAGNOSED_CONDITION_PREDICATE_ID)
        .expect("diagnosis predicate ID");
    let core_mod_id = ModId::parse("games.loreloom.core").expect("core mod id");
    let registry = DefinitionRegistry::build_packages([
        (
            ContentPackContext {
                mod_id: core_mod_id,
                mod_version: Version::new(1, 0, 0),
                pack_id: "games.loreloom.core:pack/core"
                    .parse()
                    .expect("core pack id"),
                content_version: 1,
                content_hash: parse_content_hash("c".repeat(64)).expect("core content hash"),
            },
            vec![ContentDocument {
                schema_version: CONTENT_SCHEMA_V1,
                definitions: vec![Definition::Tag(TagDefinition {
                    id: diagnosis_predicate,
                    display_name: name("Diagnosed condition"),
                })],
            }],
        ),
        (
            ContentPackContext {
                mod_id,
                mod_version: Version::new(1, 0, 0),
                pack_id: pack_id.clone(),
                content_version: 1,
                content_hash: parse_content_hash("b".repeat(64)).expect("content hash"),
            },
            vec![ContentDocument {
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
                    Definition::Character(CharacterDefinition {
                        id: preset_id.clone(),
                        display_name: name("Orin"),
                        profile: CharacterProfile {
                            summary: text("A prepared witness from the content pack."),
                            values: Vec::new(),
                            speaking_style: text("Patient and exact."),
                            narrative_tags: BTreeSet::new(),
                        },
                        agent_profile: Some(profile_id.clone()),
                        base_attributes: BaseAttributes::default(),
                        resources: Vec::new(),
                        conditions: Vec::new(),
                        inventory: Vec::new(),
                        skills: Vec::new(),
                        knowledge: Vec::new(),
                        goals: Vec::new(),
                        spawn_constraints: SpawnConstraints {
                            minimum_attributes: BTreeMap::new(),
                            maximum_attributes: BTreeMap::new(),
                            maximum_attribute_points: Fixed::ZERO,
                            maximum_items: 0,
                            maximum_skills: 0,
                            allowed_definitions: BTreeSet::new(),
                        },
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
                    Definition::Item(ItemDefinition {
                        id: stack_definition,
                        display_name: name("Trade tokens"),
                        description: text("A small stack of trade tokens."),
                        tags: BTreeSet::new(),
                        stack_limit: NonZeroU32::new(99).expect("stack limit"),
                        unit_weight_grams: Fixed::ONE,
                        durability: None,
                        container: None,
                        equipment_slots: BTreeSet::new(),
                        modifiers: Vec::new(),
                    }),
                    Definition::EquipmentSlot(EquipmentSlotDefinition {
                        id: equipment_slot.clone(),
                        display_name: name("Hand"),
                        allowed_item_tags: BTreeSet::new(),
                    }),
                    Definition::Item(ItemDefinition {
                        id: gear_definition,
                        display_name: name("Hand lantern"),
                        description: text("A lantern that occupies one hand."),
                        tags: BTreeSet::new(),
                        stack_limit: NonZeroU32::MIN,
                        unit_weight_grams: Fixed::ONE,
                        durability: None,
                        container: None,
                        equipment_slots: BTreeSet::from([equipment_slot]),
                        modifiers: Vec::new(),
                    }),
                    Definition::Resource(ResourceDefinition {
                        id: resource_definition.clone(),
                        display_name: name("Focus"),
                        minimum: Fixed::ZERO,
                        maximum: Fixed::from_integer(10).expect("resource maximum"),
                        maximum_policy: ResourceMaximumPolicy::ClampCurrent,
                        derived_from_attribute: None,
                    }),
                    Definition::Skill(SkillDefinition {
                        id: skill_definition,
                        display_name: name("Steady breath"),
                        description: text("Spend one focus to steady yourself."),
                        kind: SkillKind::Active,
                        costs: vec![ResourceCost {
                            resource_id: resource_definition,
                            amount: Fixed::ONE,
                        }],
                        cooldown_ticks: 3,
                        target: SkillTarget::SelfTarget,
                        executor_id: definition_id("skill_executor", "effects"),
                        effects: Vec::new(),
                        reaction: None,
                    }),
                    Definition::Condition(ConditionDefinition {
                        id: condition_id.clone(),
                        display_name: name("Winter fever"),
                        tags: BTreeSet::new(),
                        stack_policy: StackPolicy::Unique,
                        intensity_policy: IntensityPolicy::Keep,
                        duration: DurationPolicy::Permanent,
                        symptoms: vec![SymptomDefinition {
                            text: text("Hands tremble."),
                            minimum_intensity: Fixed::ONE,
                        }],
                        modifiers: Vec::new(),
                        periodic: None,
                    }),
                    Definition::Place(PlaceDefinition {
                        id: place_definition.clone(),
                        display_name: name("Hall"),
                        description: text("A quiet timber hall."),
                        tags: BTreeSet::new(),
                        edges: BTreeSet::new(),
                    }),
                    Definition::Place(PlaceDefinition {
                        id: forest_place_definition.clone(),
                        display_name: name("Forest Path"),
                        description: text("Mist softens the old pines."),
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
                    Definition::Scene(SceneDefinition {
                        id: forest_scene_definition.clone(),
                        display_name: name("Forest"),
                        framing: text("Mist gathers beneath the pines."),
                        entry_place: forest_place_definition.clone(),
                        places: BTreeSet::from([forest_place_definition.clone()]),
                        characters: vec![
                            SceneCharacterDefinition {
                                local_key: text("player"),
                                character_id: preset_id.clone(),
                                place_id: forest_place_definition.clone(),
                                controller: InitialCharacterController::Player,
                                lifetime: InitialCharacterLifetime::Persistent,
                            },
                            SceneCharacterDefinition {
                                local_key: text("watcher"),
                                character_id: preset_id.clone(),
                                place_id: forest_place_definition,
                                controller: InitialCharacterController::Agent,
                                lifetime: InitialCharacterLifetime::Scene,
                            },
                        ],
                    }),
                ],
            }],
        ),
    ])
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
        DomainRecord::Condition(ConditionRecord {
            id: object_id("2b47"),
            target_id: player,
            condition_id: condition_id.clone(),
            source: ConditionSource::System {
                source_id: definition_id("system", "weather"),
            },
            stacks: NonZeroU32::MIN,
            intensity: Fixed::ONE,
            applied_at: WorldTime::ZERO,
            expires_at: None,
            next_periodic_at: None,
            origin: EntityOrigin::Content {
                origin: origin(&condition_id),
            },
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
            world_lock: WorldLock {
                world_id: parse("games.loreloom.test"),
                version: parse("1.0.0"),
                content_hash: parse_content_hash("d".repeat(64)).expect("world content hash"),
                manifest_schema: 1,
                content_schema: CONTENT_SCHEMA_V1,
            },
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
        place,
        profile_id,
        preset_id,
        condition_id,
        forest_scene_definition,
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

#[derive(Debug, Clone, Copy)]
struct ToolFixtureIds {
    stack: ObjectId,
    gear: ObjectId,
    grant: ObjectId,
    npc_grant: ObjectId,
}

fn add_tool_records(fixture: &mut Fixture) -> ToolFixtureIds {
    let stack_definition = definition_id("item", "token_stack");
    let gear_definition = definition_id("item", "hand_lantern");
    let resource_definition = definition_id("resource", "focus");
    let skill_definition = definition_id("skill", "steady_breath");
    let stack = object_id("2bc0");
    let gear = object_id("2bc1");
    let grant = object_id("2bc2");
    let npc_grant = object_id("2bc5");
    let stack_origin = fixture
        .registry
        .get(&stack_definition)
        .expect("stack definition")
        .origin
        .clone();
    let gear_origin = fixture
        .registry
        .get(&gear_definition)
        .expect("gear definition")
        .origin
        .clone();
    let skill_origin = fixture
        .registry
        .get(&skill_definition)
        .expect("skill definition")
        .origin
        .clone();
    let player = fixture
        .records
        .iter_mut()
        .find_map(|record| match record {
            DomainRecord::Character(character) if character.id == fixture.player => Some(character),
            _ => None,
        })
        .expect("player character");
    player.resources.insert(
        resource_definition.clone(),
        ResourcePool {
            resource_id: resource_definition,
            current: Fixed::from_integer(5).expect("resource current"),
            base_maximum: Fixed::from_integer(10).expect("resource maximum"),
        },
    );
    let player_root = player.inventory_root;
    fixture.records.extend([
        DomainRecord::Item(ItemRecord {
            id: stack,
            definition_id: stack_definition,
            stack: StackState(NonZeroU32::new(3).expect("stack quantity")),
            durability: None,
            container: None,
            contained_by: None,
            owned_by: Some(fixture.player),
            equipped: None,
            located_at: Some(fixture.place),
            custom_name: None,
            bound_actor: None,
            parameters: BTreeMap::new(),
            instance_adjustments: Vec::new(),
            origin: EntityOrigin::Content {
                origin: stack_origin,
            },
        }),
        DomainRecord::Item(ItemRecord {
            id: gear,
            definition_id: gear_definition,
            stack: StackState(NonZeroU32::MIN),
            durability: None,
            container: None,
            contained_by: Some(player_root),
            owned_by: Some(fixture.player),
            equipped: None,
            located_at: None,
            custom_name: None,
            bound_actor: None,
            parameters: BTreeMap::new(),
            instance_adjustments: Vec::new(),
            origin: EntityOrigin::Content {
                origin: gear_origin,
            },
        }),
        DomainRecord::SkillGrant(SkillGrantRecord {
            id: grant,
            owner_id: fixture.player,
            skill_id: skill_definition.clone(),
            rank: 1,
            proficiency: 0,
            source: SkillSource::Rule {
                rule_id: definition_id("rule", "bootstrap"),
            },
            enabled: true,
            ready_at: None,
            origin: EntityOrigin::Content {
                origin: skill_origin.clone(),
            },
        }),
        DomainRecord::SkillGrant(SkillGrantRecord {
            id: npc_grant,
            owner_id: fixture.npc,
            skill_id: skill_definition,
            rank: 1,
            proficiency: 0,
            source: SkillSource::Rule {
                rule_id: definition_id("rule", "bootstrap"),
            },
            enabled: true,
            ready_at: None,
            origin: EntityOrigin::Content {
                origin: skill_origin,
            },
        }),
    ]);
    ToolFixtureIds {
        stack,
        gear,
        grant,
        npc_grant,
    }
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

fn tool_response(call: ToolCall) -> CompletionResponse {
    CompletionResponse {
        id: None,
        model: Some("mock-narrator".to_owned()),
        content: vec![AssistantContent::ToolCall(call)],
        finish_reason: Some(FinishReason::ToolCall),
        usage: None,
        provider_metadata: JsonValue::Null,
    }
}

#[tokio::test]
async fn condition_name_projection_requires_a_confirmed_diagnosis_fact() {
    for diagnosed in [false, true] {
        let directory = TempDir::new().expect("temporary save parent");
        let mut fixture = fixture();
        if diagnosed {
            fixture
                .records
                .push(DomainRecord::KnownFact(KnownFactRecord {
                    id: object_id("2b48"),
                    owner_id: fixture.player,
                    subject: FactSubject::Object {
                        object_id: fixture.player.object_id(),
                    },
                    predicate_id: ContentDefinitionId::parse(DIAGNOSED_CONDITION_PREDICATE_ID)
                        .expect("diagnosis predicate ID"),
                    value: FactValue::Tag(fixture.condition_id.clone()),
                    status: KnowledgeStatus::Confirmed,
                    confidence: Fixed::ONE,
                    source: FactSource::Content {
                        definition_id: fixture.condition_id.clone(),
                    },
                    first_known_at: WorldTime::ZERO,
                    last_confirmed_at: WorldTime::ZERO,
                }));
        }
        let candidate_world_lock = fixture.manifest.world_lock.clone();
        let candidate_mod_lock = fixture.manifest.mod_lock.clone();
        let store = SaveStore::create(
            directory.path().join("save"),
            fixture.manifest,
            fixture.records,
        )
        .await
        .expect("create diagnosis save");
        let service = WorldService::open(
            store,
            fixture.registry,
            &candidate_world_lock,
            &candidate_mod_lock,
            fixture.world_config,
        )
        .await
        .expect("open diagnosis service");
        let snapshot = service
            .snapshot(
                parse("ses_01890f6a-2b49-7d4e-8f90-123456789abc"),
                loreloom_core::RuntimePhase::Idle,
                Vec::new(),
                Vec::new(),
                Vec::new(),
            )
            .await
            .expect("diagnosis snapshot");
        let condition = snapshot
            .player
            .conditions
            .first()
            .expect("condition projection");
        assert_eq!(
            condition.display_name.as_ref().map(DisplayName::as_str),
            diagnosed.then_some("Winter fever")
        );
        assert_eq!(condition.symptoms, vec![text("Hands tremble.")]);
        let encoded = serde_json::to_string(&snapshot.player).expect("encode player context");
        assert_eq!(encoded.contains("Winter fever"), diagnosed);
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
    planning_observation: std::sync::Mutex<Option<JsonValue>>,
    saw_minimal_npc_tool: std::sync::atomic::AtomicBool,
}

impl NarratorBridge {
    fn new(plan: NarratorPlan, support: SupportMode) -> Self {
        Self {
            plan,
            support,
            calls: AtomicUsize::new(0),
            planning_observation: std::sync::Mutex::new(None),
            saw_minimal_npc_tool: std::sync::atomic::AtomicBool::new(false),
        }
    }

    fn planning_observation(&self) -> JsonValue {
        self.planning_observation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .expect("planning observation")
    }

    fn saw_minimal_npc_tool(&self) -> bool {
        self.saw_minimal_npc_tool.load(Ordering::SeqCst)
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
            self.calls.fetch_add(1, Ordering::SeqCst);
            if let Some(expected) = self.plan.npc_turns.first().map(|turn| turn.actor_id) {
                let valid = request.tools.iter().any(|definition| {
                    definition.name == "request_npc_turn"
                        && definition.input_schema["properties"]
                            .get("scene_id")
                            .is_none()
                        && definition.input_schema["properties"]["actor_id"]["enum"]
                            .as_array()
                            .is_some_and(|actors| actors.contains(&json!(expected)))
                });
                self.saw_minimal_npc_tool.store(valid, Ordering::SeqCst);
            }
            let payload = request
                .messages
                .iter()
                .rev()
                .flat_map(|message| message.content.iter())
                .find_map(|part| match part {
                    armillae_core::ContentPart::Text(text) => {
                        serde_json::from_str::<JsonValue>(&text.text).ok()
                    }
                    _ => None,
                })
                .ok_or_else(|| BridgeError::InvalidRequest {
                    message: "missing narrator payload".to_owned(),
                })?;
            if self
                .planning_observation
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_none()
            {
                *self
                    .planning_observation
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) =
                    Some(payload["payload"]["observation"].clone());
            }
            let has_tool_result = request.messages.iter().any(|message| {
                message
                    .content
                    .iter()
                    .any(|part| matches!(part, armillae_core::ContentPart::ToolResult(_)))
            });
            let has_npc_results = payload["payload"]["npc_results"]
                .as_array()
                .is_some_and(|results| !results.is_empty());
            if !has_tool_result && !has_npc_results && !self.plan.npc_turns.is_empty() {
                let content = self
                    .plan
                    .npc_turns
                    .iter()
                    .enumerate()
                    .map(|(index, npc)| {
                        AssistantContent::ToolCall(ToolCall {
                            id: ToolCallId::new(format!("request-npc-{index}"))
                                .expect("test tool call ID"),
                            name: "request_npc_turn".to_owned(),
                            arguments: json!({
                                "actor_id": npc.actor_id,
                                "assignment": npc.assignment
                            }),
                        })
                    })
                    .collect::<Vec<_>>();
                return Ok(CompletionResponse {
                    id: None,
                    model: Some("runtime-test".to_owned()),
                    content,
                    finish_reason: Some(FinishReason::ToolCall),
                    usage: None,
                    provider_metadata: JsonValue::Null,
                });
            }
            let _ = &self.support;
            Ok(text_response(if has_npc_results {
                "The inn settles into a deliberate silence."
            } else if has_tool_result {
                "The narrator waits for the requested voices."
            } else {
                "The inn settles into a deliberate silence."
            }))
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

struct RecoveringNarratorBridge {
    calls: AtomicUsize,
}

struct RejectedNpcBridge;

impl LlmBridge for RejectedNpcBridge {
    fn capabilities(&self) -> BridgeCapabilities {
        BridgeCapabilities::all()
    }

    fn project(&self, _request: &CompletionRequest) -> Result<ProjectionReport, BridgeError> {
        Ok(ProjectionReport::exact("runtime-rejected-npc"))
    }

    fn complete<'a>(
        &'a self,
        _request: CompletionRequest,
    ) -> BridgeFuture<'a, Result<CompletionResponse, BridgeError>> {
        Box::pin(async {
            Err(BridgeError::Timeout {
                metadata: ErrorMetadata::new("openai-compatible").with_request_id("req_npc-safe"),
            })
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

impl LlmBridge for RecoveringNarratorBridge {
    fn capabilities(&self) -> BridgeCapabilities {
        BridgeCapabilities::all()
    }

    fn project(&self, _request: &CompletionRequest) -> Result<ProjectionReport, BridgeError> {
        Ok(ProjectionReport::exact("runtime-recovering-narrator"))
    }

    fn complete<'a>(
        &'a self,
        _request: CompletionRequest,
    ) -> BridgeFuture<'a, Result<CompletionResponse, BridgeError>> {
        Box::pin(async move {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                return Err(BridgeError::ProviderRejected {
                    code: Some("must-not-escape".to_owned()),
                    message: "provider rejected credential must-not-escape".to_owned(),
                    metadata: ErrorMetadata::new("openai-compatible")
                        .with_http_status(400)
                        .with_request_id("req_runtime-safe"),
                });
            }
            Ok(text_response(
                "The rain eases, and the story continues without changing the world.",
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

struct GeneratedNarratorBridge {
    profile_id: ContentDefinitionId,
    calls: AtomicUsize,
    saw_materialized_profile: std::sync::atomic::AtomicBool,
    repeat_materialization: bool,
    materialized_actor: std::sync::Mutex<Option<(ActorId, Revision)>>,
}

impl GeneratedNarratorBridge {
    fn new(profile_id: ContentDefinitionId) -> Self {
        Self {
            profile_id,
            calls: AtomicUsize::new(0),
            saw_materialized_profile: std::sync::atomic::AtomicBool::new(false),
            repeat_materialization: false,
            materialized_actor: std::sync::Mutex::new(None),
        }
    }

    fn repeating(profile_id: ContentDefinitionId) -> Self {
        Self {
            repeat_materialization: true,
            ..Self::new(profile_id)
        }
    }

    fn creation_request(&self) -> CreateNpcRequest {
        CreateNpcRequest {
            source: NpcCreationSource::Generated {
                role: text("witness"),
                purpose: LongText::new("Create a witness who can answer the player's question.")
                    .expect("generation purpose"),
            },
            lifetime: NpcLifetime::Scene,
            mode: NpcCreationMode::Agent,
        }
    }

    fn npc_turn_response(&self, actor_id: ActorId) -> CompletionResponse {
        tool_response(ToolCall {
            id: ToolCallId::new("request-generated-witness").expect("request tool call ID"),
            name: "request_npc_turn".to_owned(),
            arguments: json!({
                "actor_id": actor_id,
                "assignment": "Answer according to the character that now exists."
            }),
        })
    }

    fn draft(&self) -> NpcDraft {
        NpcDraft {
            display_name: name("Ilya"),
            profile: CharacterProfile {
                summary: text("A rain-soaked witness with a careful memory."),
                values: Vec::new(),
                speaking_style: text("Careful and concrete."),
                narrative_tags: BTreeSet::new(),
            },
            agent_profile: Some(self.profile_id.clone()),
            base_attributes: BaseAttributes::default(),
            resources: Vec::new(),
            conditions: Vec::new(),
            inventory: Vec::new(),
            skills: Vec::new(),
            knowledge: Vec::new(),
            goals: Vec::new(),
        }
    }
}

impl LlmBridge for GeneratedNarratorBridge {
    fn capabilities(&self) -> BridgeCapabilities {
        BridgeCapabilities::all()
    }

    fn project(&self, _request: &CompletionRequest) -> Result<ProjectionReport, BridgeError> {
        Ok(ProjectionReport::exact("runtime-generation-test"))
    }

    fn complete<'a>(
        &'a self,
        request: CompletionRequest,
    ) -> BridgeFuture<'a, Result<CompletionResponse, BridgeError>> {
        Box::pin(async move {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            match call {
                0 => Ok(tool_response(ToolCall {
                    id: ToolCallId::new("materialize-witness").expect("tool call ID"),
                    name: "create_npc".to_owned(),
                    arguments: serde_json::to_value(self.creation_request())
                        .expect("materialization arguments"),
                })),
                1 => Ok(text_response("The witness request is queued.")),
                2 => Ok(tool_response(ToolCall {
                    id: ToolCallId::new("submit-witness-draft").expect("draft tool call ID"),
                    name: "submit_npc_draft".to_owned(),
                    arguments: serde_json::to_value(self.draft()).expect("generated draft"),
                })),
                3 => Ok(text_response("The generated draft is ready.")),
                4 => {
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
                            message: "missing replanning payload".to_owned(),
                        })?;
                    let result = &payload["payload"]["materialization_results"][0];
                    let actor_id = serde_json::from_value::<ActorId>(result["actor_id"].clone())
                        .map_err(|_| BridgeError::InvalidRequest {
                            message: "missing materialized actor".to_owned(),
                        })?;
                    let revision = serde_json::from_value::<Revision>(result["revision"].clone())
                        .map_err(|_| BridgeError::InvalidRequest {
                        message: "missing materialized revision".to_owned(),
                    })?;
                    let visible = payload["payload"]["observation"]["scene"]["visible_actors"]
                        .as_array()
                        .is_some_and(|actors| {
                            actors.iter().any(|actor| {
                                actor["actor_id"] == json!(actor_id)
                                    && actor["display_name"] == json!("Ilya")
                            })
                        });
                    self.saw_materialized_profile
                        .store(visible, Ordering::SeqCst);
                    if self.repeat_materialization {
                        *self
                            .materialized_actor
                            .lock()
                            .expect("materialized actor lock") = Some((actor_id, revision));
                        Ok(tool_response(ToolCall {
                            id: ToolCallId::new("repeat-materialize-witness")
                                .expect("repeat tool call ID"),
                            name: "create_npc".to_owned(),
                            arguments: serde_json::to_value(self.creation_request())
                                .expect("repeated materialization arguments"),
                        }))
                    } else {
                        Ok(self.npc_turn_response(actor_id))
                    }
                }
                5 => {
                    if self.repeat_materialization {
                        let (actor_id, _) = self
                            .materialized_actor
                            .lock()
                            .expect("materialized actor lock")
                            .expect("materialized actor");
                        return Ok(self.npc_turn_response(actor_id));
                    }
                    Ok(text_response("The generated NPC turn is queued."))
                }
                6 if self.repeat_materialization => {
                    Ok(text_response("The generated NPC turn is queued."))
                }
                6 | 7 => Ok(text_response(
                    "Ilya answers only after becoming part of the world.",
                )),
                _ => Err(BridgeError::InvalidRequest {
                    message: "unexpected narrator call".to_owned(),
                }),
            }
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

struct PresetNarratorBridge {
    character_id: ContentDefinitionId,
    calls: AtomicUsize,
    saw_materialized_profile: std::sync::atomic::AtomicBool,
}

impl PresetNarratorBridge {
    fn new(character_id: ContentDefinitionId) -> Self {
        Self {
            character_id,
            calls: AtomicUsize::new(0),
            saw_materialized_profile: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

impl LlmBridge for PresetNarratorBridge {
    fn capabilities(&self) -> BridgeCapabilities {
        BridgeCapabilities::all()
    }

    fn project(&self, _request: &CompletionRequest) -> Result<ProjectionReport, BridgeError> {
        Ok(ProjectionReport::exact("runtime-preset-test"))
    }

    fn complete<'a>(
        &'a self,
        request: CompletionRequest,
    ) -> BridgeFuture<'a, Result<CompletionResponse, BridgeError>> {
        Box::pin(async move {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            match call {
                0 => Ok(tool_response(ToolCall {
                    id: ToolCallId::new("materialize-preset").expect("tool call ID"),
                    name: "create_npc".to_owned(),
                    arguments: serde_json::to_value(CreateNpcRequest {
                        source: NpcCreationSource::Preset {
                            character_id: self.character_id.clone(),
                        },
                        lifetime: NpcLifetime::Persistent,
                        mode: NpcCreationMode::Agent,
                    })
                    .expect("preset decision"),
                })),
                1 => Ok(text_response("The preset character request is queued.")),
                2 => {
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
                            message: "missing preset replanning payload".to_owned(),
                        })?;
                    let result = &payload["payload"]["materialization_results"][0];
                    let actor_id = serde_json::from_value::<ActorId>(result["actor_id"].clone())
                        .map_err(|_| BridgeError::InvalidRequest {
                            message: "missing preset actor".to_owned(),
                        })?;
                    let visible = payload["payload"]["observation"]["scene"]["visible_actors"]
                        .as_array()
                        .is_some_and(|actors| {
                            actors.iter().any(|actor| {
                                actor["actor_id"] == json!(actor_id)
                                    && actor["display_name"] == json!("Orin")
                            })
                        });
                    self.saw_materialized_profile
                        .store(visible, Ordering::SeqCst);
                    Ok(tool_response(ToolCall {
                        id: ToolCallId::new("request-preset-npc").expect("request tool call ID"),
                        name: "request_npc_turn".to_owned(),
                        arguments: json!({
                            "actor_id": actor_id,
                            "assignment": "Respond as the fully loaded preset character."
                        }),
                    }))
                }
                3 => Ok(text_response("The preset NPC turn is queued.")),
                4 => Ok(text_response(
                    "Orin joins the conversation from the prepared cast.",
                )),
                _ => Err(BridgeError::InvalidRequest {
                    message: "unexpected preset narrator call".to_owned(),
                }),
            }
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

#[derive(Clone, Copy)]
enum GenerationRejectionMode {
    ResourceLimit,
    ProviderFailure,
}

struct RejectedGenerationBridge {
    mode: GenerationRejectionMode,
    calls: AtomicUsize,
    saw_rejection: std::sync::atomic::AtomicBool,
}

impl RejectedGenerationBridge {
    fn new(mode: GenerationRejectionMode) -> Self {
        Self {
            mode,
            calls: AtomicUsize::new(0),
            saw_rejection: std::sync::atomic::AtomicBool::new(false),
        }
    }

    fn creation_request(&self) -> CreateNpcRequest {
        CreateNpcRequest {
            source: NpcCreationSource::Generated {
                role: text("witness"),
                purpose: LongText::new("Create one bounded witness.").expect("generation purpose"),
            },
            lifetime: NpcLifetime::Scene,
            mode: NpcCreationMode::Agent,
        }
    }
}

impl LlmBridge for RejectedGenerationBridge {
    fn capabilities(&self) -> BridgeCapabilities {
        BridgeCapabilities::all()
    }

    fn project(&self, _request: &CompletionRequest) -> Result<ProjectionReport, BridgeError> {
        Ok(ProjectionReport::exact("runtime-rejected-generation"))
    }

    fn complete<'a>(
        &'a self,
        request: CompletionRequest,
    ) -> BridgeFuture<'a, Result<CompletionResponse, BridgeError>> {
        Box::pin(async move {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            if call == 0 {
                return Ok(tool_response(ToolCall {
                    id: ToolCallId::new("rejected-generation").expect("tool call ID"),
                    name: "create_npc".to_owned(),
                    arguments: serde_json::to_value(self.creation_request())
                        .expect("generation request"),
                }));
            }
            if call == 1 {
                return Ok(text_response(
                    serde_json::to_string(&NarratorPlan {
                        based_on_revision: Revision::new(1),
                        npc_turns: Vec::new(),
                    })
                    .expect("provisional rejection plan"),
                ));
            }
            if matches!(self.mode, GenerationRejectionMode::ProviderFailure) && call == 2 {
                return Err(BridgeError::ProviderRejected {
                    code: Some("must-not-escape".to_owned()),
                    message: "injected generation failure must-not-escape".to_owned(),
                    metadata: ErrorMetadata::new("openai-compatible")
                        .with_http_status(422)
                        .with_request_id("req_generation-safe"),
                });
            }
            let replanning_call = match self.mode {
                GenerationRejectionMode::ResourceLimit => 2,
                GenerationRejectionMode::ProviderFailure => 3,
            };
            if call == replanning_call {
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
                        message: "missing rejection replanning payload".to_owned(),
                    })?;
                let result = &payload["payload"]["materialization_results"][0];
                let expected_reason = match self.mode {
                    GenerationRejectionMode::ResourceLimit => "generation_limit",
                    GenerationRejectionMode::ProviderFailure => "bridge_unavailable",
                };
                self.saw_rejection.store(
                    result["status"] == json!("rejected")
                        && result["reason"] == json!(expected_reason)
                        && result.get("actor_id").is_none()
                        && (!matches!(self.mode, GenerationRejectionMode::ProviderFailure)
                            || (result["failure"]["category"] == json!("provider_rejected")
                                && result["failure"]["http_status"] == json!(422)
                                && result["failure"]["provider"] == json!("openai-compatible")
                                && result["failure"]["request_id"]
                                    == json!("req_generation-safe")
                                && result["failure"]["correlation_id"]
                                    .as_str()
                                    .is_some_and(|value| value.starts_with("err_")))),
                    Ordering::SeqCst,
                );
                return Ok(text_response(
                    serde_json::to_string(&NarratorPlan {
                        based_on_revision: Revision::new(1),
                        npc_turns: Vec::new(),
                    })
                    .expect("recovery plan"),
                ));
            }
            if call == replanning_call + 1 {
                return Ok(text_response(
                    serde_json::to_string(&json!({
                        "kind": "final",
                        "based_on_revision": 1,
                        "narration": "No new witness enters the scene.",
                        "supporting_events": []
                    }))
                    .expect("rejection synthesis"),
                ));
            }
            Err(BridgeError::InvalidRequest {
                message: "unexpected rejected generation call".to_owned(),
            })
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

struct CancellableGenerationBridge {
    request: CreateNpcRequest,
    calls: AtomicUsize,
    entered_generation: Arc<tokio::sync::Notify>,
}

impl LlmBridge for CancellableGenerationBridge {
    fn capabilities(&self) -> BridgeCapabilities {
        BridgeCapabilities::all()
    }

    fn project(&self, _request: &CompletionRequest) -> Result<ProjectionReport, BridgeError> {
        Ok(ProjectionReport::exact("runtime-cancellable-generation"))
    }

    fn complete<'a>(
        &'a self,
        _request: CompletionRequest,
    ) -> BridgeFuture<'a, Result<CompletionResponse, BridgeError>> {
        Box::pin(async move {
            match self.calls.fetch_add(1, Ordering::SeqCst) {
                0 => Ok(tool_response(ToolCall {
                    id: ToolCallId::new("cancelled-generation").expect("tool call ID"),
                    name: "create_npc".to_owned(),
                    arguments: serde_json::to_value(&self.request)
                        .expect("cancelled generation request"),
                })),
                1 => Ok(text_response(
                    serde_json::to_string(&NarratorPlan {
                        based_on_revision: Revision::new(1),
                        npc_turns: Vec::new(),
                    })
                    .expect("cancel provisional plan"),
                )),
                2 => {
                    self.entered_generation.notify_one();
                    std::future::pending::<Result<CompletionResponse, BridgeError>>().await
                }
                _ => Err(BridgeError::InvalidRequest {
                    message: "unexpected cancellation call".to_owned(),
                }),
            }
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

fn narrator_definition() -> NarratorDefinition {
    NarratorDefinition {
        narrator_prompts: vec![LongText::new("Narrate the test world.").expect("narrator prompt")],
        npc_prompts: vec![LongText::new("Respect the shared test lore.").expect("NPC prompt")],
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
        MockResponse::text("One moment passes. I wait by the hearth."),
    ]))
}

fn generation_policy(
    policy_id: ContentDefinitionId,
    profile_id: ContentDefinitionId,
) -> GenerationPolicy {
    GenerationPolicy {
        id: policy_id,
        constraints: SpawnConstraints {
            minimum_attributes: BTreeMap::new(),
            maximum_attributes: BTreeMap::new(),
            maximum_attribute_points: Fixed::ZERO,
            maximum_items: 0,
            maximum_skills: 0,
            allowed_definitions: BTreeSet::new(),
        },
        allowed_agent_profiles: BTreeSet::from([profile_id]),
    }
}

#[tokio::test]
async fn world_service_adopts_a_compatible_candidate_mod_lock() {
    let directory = TempDir::new().expect("temporary save parent");
    let fixture = fixture();
    let candidate_world = fixture.manifest.world_lock.clone();
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

    let service = WorldService::open(
        store,
        fixture.registry,
        &candidate_world,
        &candidate,
        fixture.world_config,
    )
    .await
    .expect("compatible candidate Mod lock");
    assert_eq!(service.revision().await, Revision::ZERO);
}

#[tokio::test]
async fn world_service_adopts_a_compatible_candidate_world_lock() {
    let directory = TempDir::new().expect("temporary save parent");
    let fixture = fixture();
    let mut candidate_world = fixture.manifest.world_lock.clone();
    candidate_world.content_hash =
        parse_content_hash("e".repeat(64)).expect("different world content hash");
    let candidate_mods = fixture.manifest.mod_lock.clone();
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
        &candidate_world,
        &candidate_mods,
        fixture.world_config,
    )
    .await
    .expect("compatible candidate World lock");
    assert_eq!(service.revision().await, Revision::ZERO);
}

#[tokio::test]
async fn world_service_rejects_a_different_world_identity() {
    let directory = TempDir::new().expect("temporary save parent");
    let fixture = fixture();
    let mut candidate_world = fixture.manifest.world_lock.clone();
    candidate_world.world_id = ModId::parse("games.loreloom.different").expect("different world");
    let candidate_mods = fixture.manifest.mod_lock.clone();
    let store = SaveStore::create(
        directory.path().join("save"),
        fixture.manifest,
        fixture.records,
    )
    .await
    .expect("create save");

    assert!(matches!(
        WorldService::open(
            store,
            fixture.registry,
            &candidate_world,
            &candidate_mods,
            fixture.world_config
        )
        .await,
        Err(RuntimeError::ContentLockMismatch)
    ));
}

#[tokio::test]
async fn failed_candidate_rebuild_preserves_the_durable_content_locks() {
    let directory = TempDir::new().expect("temporary save parent");
    let fixture = fixture();
    let original_manifest = fixture.manifest.clone();
    let missing_definition = definition_id("attribute", "removed");
    let mut records = fixture.records;
    let player = records
        .iter_mut()
        .find_map(|record| match record {
            DomainRecord::Character(character) if character.id == fixture.player => Some(character),
            _ => None,
        })
        .expect("player record");
    player
        .base_attributes
        .0
        .insert(missing_definition.clone(), Fixed::ONE);
    let mut candidate_world = original_manifest.world_lock.clone();
    candidate_world.content_hash =
        parse_content_hash("e".repeat(64)).expect("different world content hash");
    let path = directory.path().join("save");
    let store = SaveStore::create(&path, original_manifest.clone(), records)
        .await
        .expect("create save");
    let mut observer = store.connect().await.expect("observer connection");

    assert!(matches!(
        WorldService::open(
            store,
            fixture.registry,
            &candidate_world,
            &original_manifest.mod_lock,
            fixture.world_config
        )
        .await,
        Err(RuntimeError::World(
            loreloom_world::WorldError::DefinitionNotFound { id }
        )) if id == missing_definition
    ));
    let loaded = observer.load().await.expect("load unchanged manifest");
    assert_eq!(loaded.manifest, original_manifest);
}

#[tokio::test]
async fn provider_failure_is_redacted_and_the_same_runtime_accepts_the_next_turn() {
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
        fixture.registry,
        &fixture.manifest.world_lock,
        &fixture.manifest.mod_lock,
        fixture.world_config,
    )
    .await
    .expect("open world service");
    let narrator = Arc::new(RecoveringNarratorBridge {
        calls: AtomicUsize::new(0),
    });
    let mut runtime = GameRuntime::new(
        Arc::clone(&service),
        narrator,
        narrator_definition(),
        parse("ses_01890f6a-2b8a-7d4e-8f90-123456789abc"),
        RuntimeConfig::default(),
    );

    let error = runtime
        .handle_player_input("Listen for the rain.")
        .await
        .expect_err("the injected provider failure must end the first turn");
    let RuntimeError::BridgeUnavailable(diagnostic) = &error else {
        panic!("expected a model failure diagnostic, got {error:?}");
    };
    assert_eq!(diagnostic.invocation, ModelInvocationKind::Narrator);
    assert_eq!(diagnostic.stage, ModelFailureStage::Invocation);
    assert_eq!(diagnostic.category, ModelFailureCategory::ProviderRejected);
    assert_eq!(diagnostic.http_status, Some(400));
    assert_eq!(
        diagnostic.provider.as_ref().map(|value| value.as_str()),
        Some("openai-compatible")
    );
    assert_eq!(
        diagnostic.request_id.as_ref().map(|value| value.as_str()),
        Some("req_runtime-safe")
    );
    assert!(diagnostic.correlation_id.to_string().starts_with("err_"));
    assert!(!error.to_string().contains("must-not-escape"));
    assert!(!format!("{error:?}").contains("must-not-escape"));

    let recoverable = runtime
        .initial_snapshot()
        .await
        .expect("the committed world remains readable");
    assert_eq!(recoverable.revision, Revision::new(1));
    assert_eq!(recoverable.transcript.items.len(), 1);
    assert!(service.events().await.is_empty());

    let completed = runtime
        .handle_player_input("Continue after the interruption.")
        .await
        .expect("the same runtime accepts a later turn");
    assert_eq!(completed.snapshot.revision, Revision::new(3));
    assert_eq!(completed.snapshot.transcript.items.len(), 3);
    assert!(completed.snapshot.can_submit);
    assert!(service.events().await.is_empty());
}

#[tokio::test]
async fn narrator_discovers_scene_targets_and_invented_targets_are_actionably_rejected() {
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
        fixture.registry,
        &fixture.manifest.world_lock,
        &fixture.manifest.mod_lock,
        fixture.world_config,
    )
    .await
    .expect("open world service");
    let executor = RuntimeToolExecutor::new(service);
    let context = || {
        ToolContext::new().with_extension(AgentToolContext {
            actor_id: fixture.player,
            revision: Revision::ZERO,
            session_id: parse("ses_01890f6a-2b87-7d4e-8f90-123456789abc"),
            capabilities: BTreeSet::from(["narrator.transition_scene".to_owned()]),
        })
    };

    let listed = executor
        .execute(
            context(),
            ToolCall {
                id: ToolCallId::new("list-scene-targets").expect("tool call ID"),
                name: "list_scene_transitions".to_owned(),
                arguments: json!({}),
            },
        )
        .await
        .expect("correlated scene target query");
    assert!(!listed.is_error);
    let listed = tool_result_json(&listed);
    assert_eq!(listed["current"]["scene_id"], json!(fixture.scene));
    assert_eq!(listed["targets"].as_array().map(Vec::len), Some(1));
    let forest_target = listed["targets"][0]["target"].clone();
    assert_eq!(
        forest_target,
        json!({
            "type": "definition",
            "scene_definition_id": fixture.forest_scene_definition
        })
    );

    let rejected = executor
        .execute(
            context(),
            ToolCall {
                id: ToolCallId::new("invented-scene-target").expect("tool call ID"),
                name: "transition_scene".to_owned(),
                arguments: json!({
                    "target": {
                        "type": "definition",
                        "scene_definition_id": "米尔港"
                    }
                }),
            },
        )
        .await
        .expect("correlated scene target rejection");
    assert!(rejected.is_error);
    assert_eq!(
        tool_result_json(&rejected),
        &json!({
            "code": "scene_transition_target_unavailable",
            "recovery_tool": "list_scene_transitions",
            "retry_unchanged": false
        })
    );

    let accepted = executor
        .execute(
            context(),
            ToolCall {
                id: ToolCallId::new("accepted-scene-target").expect("tool call ID"),
                name: "transition_scene".to_owned(),
                arguments: json!({ "target": forest_target.clone() }),
            },
        )
        .await
        .expect("correlated scene transition acceptance");
    assert!(!accepted.is_error);
    let duplicate = executor
        .execute(
            context(),
            ToolCall {
                id: ToolCallId::new("duplicate-scene-target").expect("tool call ID"),
                name: "transition_scene".to_owned(),
                arguments: json!({ "target": forest_target }),
            },
        )
        .await
        .expect("correlated duplicate scene transition acceptance");
    assert!(!duplicate.is_error);
    assert_eq!(tool_result_json(&duplicate)["duplicate"], json!(true));
}

#[tokio::test]
async fn narrator_scene_transition_materializes_replans_and_revisits_without_regeneration() {
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
        fixture.registry,
        &fixture.manifest.world_lock,
        &fixture.manifest.mod_lock,
        fixture.world_config,
    )
    .await
    .expect("open world service");
    let narrator = Arc::new(MockBridge::scripted([
        MockResponse::tool_call(
            ToolCallId::new("list-before-forest").expect("tool call ID"),
            "list_scene_transitions",
            json!({}),
        ),
        MockResponse::tool_call(
            ToolCallId::new("enter-forest").expect("tool call ID"),
            "transition_scene",
            json!({
                "target": {
                    "type": "definition",
                    "scene_definition_id": fixture.forest_scene_definition
                }
            }),
        ),
        MockResponse::text("I will lead the story into the forest."),
        MockResponse::text("Mist gathers as the forest path opens before you."),
        MockResponse::tool_call(
            ToolCallId::new("list-before-inn").expect("tool call ID"),
            "list_scene_transitions",
            json!({}),
        ),
        MockResponse::tool_call(
            ToolCallId::new("return-inn").expect("tool call ID"),
            "transition_scene",
            json!({
                "target": {
                    "type": "existing",
                    "scene_id": fixture.scene
                }
            }),
        ),
        MockResponse::text("I will return the story to the inn."),
        MockResponse::text("The inn receives you with rain against its shutters."),
    ]));
    let mut runtime = GameRuntime::new(
        Arc::clone(&service),
        narrator,
        narrator_definition(),
        parse("ses_01890f6a-2b88-7d4e-8f90-123456789abc"),
        RuntimeConfig::default(),
    );

    let first = runtime
        .handle_player_input("Take the forest road.")
        .await
        .expect("enter forest");
    assert_eq!(first.snapshot.scene.display_name.as_str(), "Forest");
    assert_eq!(first.snapshot.revision, Revision::new(3));
    let forest_scene = first.snapshot.scene.scene_id;
    let forest_npc = first
        .snapshot
        .scene
        .visible_actors
        .iter()
        .find(|actor| actor.actor_id != fixture.player)
        .map(|actor| actor.actor_id)
        .expect("materialized forest NPC");

    let second = runtime
        .handle_player_input("Return to the inn.")
        .await
        .expect("return to inn");
    assert_eq!(second.snapshot.scene.scene_id, fixture.scene);
    assert_eq!(second.snapshot.revision, Revision::new(6));

    let transition_context = AgentToolContext {
        actor_id: fixture.player,
        revision: Revision::new(6),
        session_id: parse("ses_01890f6a-2b88-7d4e-8f90-123456789abc"),
        capabilities: BTreeSet::from(["narrator.transition_scene".to_owned()]),
    };
    let executor = RuntimeToolExecutor::new(Arc::clone(&service));
    let listed = executor
        .execute(
            ToolContext::new().with_extension(transition_context.clone()),
            ToolCall {
                id: ToolCallId::new("list-materialized-forest").expect("tool call ID"),
                name: "list_scene_transitions".to_owned(),
                arguments: json!({}),
            },
        )
        .await
        .expect("list materialized forest target");
    let targets = tool_result_json(&listed)["targets"]
        .as_array()
        .expect("scene transition targets");
    assert_eq!(
        targets.len(),
        1,
        "materialized scenes replace definition targets"
    );
    let forest_target = targets
        .iter()
        .find(|option| option["target"]["scene_id"] == json!(forest_scene))
        .map(|option| option["target"].clone())
        .expect("materialized forest is returned by existing Scene ID");
    assert_eq!(forest_target["type"], json!("existing"));
    service
        .execute(
            &transition_context,
            WorldCommandKind::TransitionScene {
                target: serde_json::from_value(forest_target).expect("listed transition target"),
            },
        )
        .await
        .expect("re-enter forest");
    let third = service
        .snapshot(
            transition_context.session_id,
            loreloom_core::RuntimePhase::Completed,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .await
        .expect("snapshot revisited forest");
    assert_eq!(third.scene.scene_id, forest_scene);
    assert_eq!(third.revision, Revision::new(7));
    assert!(
        third
            .scene
            .visible_actors
            .iter()
            .any(|actor| actor.actor_id == forest_npc),
        "revisiting restores the original scene-owned NPC"
    );
    let events = service.events().await;
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.kind, WorldEventKind::SceneEntered { .. }))
            .count(),
        3
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.kind, WorldEventKind::SceneLeft { .. }))
            .count(),
        3
    );
}

#[tokio::test]
async fn scene_transition_is_mutually_exclusive_with_scene_bound_npc_orchestration() {
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
        fixture.registry,
        &fixture.manifest.world_lock,
        &fixture.manifest.mod_lock,
        fixture.world_config,
    )
    .await
    .expect("open world service");
    let context = || {
        ToolContext::new().with_extension(AgentToolContext {
            actor_id: fixture.player,
            revision: Revision::ZERO,
            session_id: parse("ses_01890f6a-2b89-7d4e-8f90-123456789abc"),
            capabilities: BTreeSet::from([
                "narrator.request_npc_turn".to_owned(),
                "narrator.transition_scene".to_owned(),
            ]),
        })
    };

    let npc_first = RuntimeToolExecutor::new(Arc::clone(&service));
    let accepted_npc = npc_first
        .execute(
            context(),
            ToolCall {
                id: ToolCallId::new("npc-before-transition").expect("tool call ID"),
                name: "request_npc_turn".to_owned(),
                arguments: json!({
                    "actor_id": fixture.npc,
                    "assignment": "Remain in the current scene."
                }),
            },
        )
        .await
        .expect("correlated NPC request");
    assert!(!accepted_npc.is_error);
    let rejected_transition = npc_first
        .execute(
            context(),
            ToolCall {
                id: ToolCallId::new("transition-after-npc").expect("tool call ID"),
                name: "transition_scene".to_owned(),
                arguments: json!({
                    "target": {
                        "type": "definition",
                        "scene_definition_id": fixture.forest_scene_definition
                    }
                }),
            },
        )
        .await
        .expect("correlated transition rejection");
    assert!(rejected_transition.is_error);

    let transition_first = RuntimeToolExecutor::new(service);
    let accepted_transition = transition_first
        .execute(
            context(),
            ToolCall {
                id: ToolCallId::new("transition-before-npc").expect("tool call ID"),
                name: "transition_scene".to_owned(),
                arguments: json!({
                    "target": {
                        "type": "definition",
                        "scene_definition_id": fixture.forest_scene_definition
                    }
                }),
            },
        )
        .await
        .expect("correlated transition request");
    assert!(!accepted_transition.is_error);
    let rejected_npc = transition_first
        .execute(
            context(),
            ToolCall {
                id: ToolCallId::new("npc-after-transition").expect("tool call ID"),
                name: "request_npc_turn".to_owned(),
                arguments: json!({
                    "actor_id": fixture.npc,
                    "assignment": "This request must wait for replanning."
                }),
            },
        )
        .await
        .expect("correlated NPC rejection");
    assert!(rejected_npc.is_error);
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
        &fixture.manifest.world_lock,
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
        narrator.clone(),
        narrator_definition(),
        parse::<SessionId>("ses_01890f6a-2b61-7d4e-8f90-123456789abc"),
        RuntimeConfig::default(),
    );
    runtime.register_npc(
        fixture.npc,
        definition(fixture.profile_id.clone()),
        npc.clone(),
    );

    let mut phases = Vec::new();
    let outcome = runtime
        .handle_player_input_with_phase("Ask Mira to listen to the rain.", |phase| {
            phases.push(phase);
        })
        .await
        .expect("complete player turn");

    assert_eq!(
        phases.first(),
        Some(&loreloom_core::RuntimePhase::PersistingInput)
    );
    assert!(phases.contains(&loreloom_core::RuntimePhase::NarratorThinking));
    assert!(phases.contains(&loreloom_core::RuntimePhase::ResolvingOrchestration));
    assert!(phases.contains(&loreloom_core::RuntimePhase::NpcThinking));
    assert!(narrator.saw_minimal_npc_tool());
    assert!(
        narrator.planning_observation()["scene"]["visible_actors"]
            .as_array()
            .is_some_and(|actors| actors.iter().any(|actor| {
                actor["actor_id"] == json!(fixture.npc)
                    && actor["npc_turn_available"] == json!(true)
            }))
    );
    assert_eq!(
        phases.last(),
        Some(&loreloom_core::RuntimePhase::UpdatingWorld)
    );

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
    let npc_requests = npc.requests().expect("npc requests");
    assert_eq!(npc_requests.len(), 2);
    assert_eq!(npc_requests[0].messages.len(), 4);
    let message_text = |index: usize| match &npc_requests[0].messages[index].content[..] {
        [armillae_core::ContentPart::Text(text)] => text.text.as_str(),
        content => panic!("expected one text part, got {content:?}"),
    };
    assert!(message_text(0).contains("tool rules"));
    assert_eq!(message_text(1), "Be concise.");
    assert_eq!(message_text(2), "Respect the shared test lore.");
    assert!(message_text(3).contains("\"kind\":\"npc_turn\""));

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
async fn npc_provider_failure_reaches_narrator_result_and_player_notice() {
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
        &fixture.manifest.world_lock,
        &fixture.manifest.mod_lock,
        fixture.world_config.clone(),
    )
    .await
    .expect("open world service");
    let plan = NarratorPlan {
        based_on_revision: Revision::new(1),
        npc_turns: vec![request(fixture.npc, fixture.scene)],
    };
    let mut runtime = GameRuntime::new(
        service,
        Arc::new(NarratorBridge::new(plan, SupportMode::Empty)),
        narrator_definition(),
        parse("ses_01890f6a-2b65-7d4e-8f90-123456789abc"),
        RuntimeConfig::default(),
    );
    runtime.register_npc(
        fixture.npc,
        definition(fixture.profile_id),
        Arc::new(RejectedNpcBridge),
    );

    let outcome = runtime
        .handle_player_input("Ask Mira to answer despite the distant service.")
        .await
        .expect("narrator can synthesize after an NPC failure");

    assert_eq!(outcome.npc_results.len(), 1);
    assert_eq!(outcome.npc_results[0].status, NpcTurnStatus::Failed);
    let diagnostic = outcome.npc_results[0]
        .failure
        .as_ref()
        .expect("NPC diagnostic");
    assert_eq!(diagnostic.invocation, ModelInvocationKind::Npc);
    assert_eq!(diagnostic.category, ModelFailureCategory::Timeout);
    assert_eq!(
        diagnostic.request_id.as_ref().map(|value| value.as_str()),
        Some("req_npc-safe")
    );
    let correlation_id = diagnostic.correlation_id.to_string();
    assert!(outcome.snapshot.notices.iter().any(|notice| {
        notice.message.as_str().contains("npc/invocation")
            && notice.message.as_str().contains("timeout")
            && notice.message.as_str().contains(&correlation_id)
    }));
}

#[tokio::test]
async fn generated_npc_is_committed_before_narrator_replans_and_dispatches_it() {
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
        &fixture.manifest.world_lock,
        &fixture.manifest.mod_lock,
        fixture.world_config.clone(),
    )
    .await
    .expect("open service");
    let policy_id = definition_id("generation_policy", "witness");
    let policy = GenerationPolicy {
        id: policy_id.clone(),
        constraints: SpawnConstraints {
            minimum_attributes: BTreeMap::new(),
            maximum_attributes: BTreeMap::new(),
            maximum_attribute_points: Fixed::ZERO,
            maximum_items: 0,
            maximum_skills: 0,
            allowed_definitions: BTreeSet::new(),
        },
        allowed_agent_profiles: BTreeSet::from([fixture.profile_id.clone()]),
    };
    let narrator = Arc::new(GeneratedNarratorBridge::new(fixture.profile_id.clone()));
    let npc = Arc::new(MockBridge::scripted([MockResponse::text(
        "I saw the lantern before the rain, and I will answer truthfully.",
    )]));
    let config = RuntimeConfig {
        generation_policy: Some(policy),
        ..RuntimeConfig::default()
    };
    let mut runtime = GameRuntime::new(
        Arc::clone(&service),
        narrator.clone(),
        narrator_definition(),
        parse("ses_01890f6a-2ba1-7d4e-8f90-123456789abc"),
        config,
    );
    runtime.set_default_npc_bridge(npc.clone());

    let outcome = runtime
        .handle_player_input("Ask whether anyone saw the lantern.")
        .await
        .expect("complete generated NPC turn");

    assert!(narrator.saw_materialized_profile.load(Ordering::SeqCst));
    assert_eq!(narrator.calls.load(Ordering::SeqCst), 7);
    assert_eq!(npc.requests().expect("NPC request log").len(), 1);
    assert_eq!(outcome.npc_results.len(), 1);
    assert_eq!(outcome.npc_results[0].status, NpcTurnStatus::Completed);
    assert_eq!(
        outcome.npc_results[0].observed_revision,
        Some(Revision::new(2))
    );
    assert_eq!(outcome.snapshot.revision, Revision::new(3));
    assert_eq!(
        outcome.narration.as_str(),
        "Ilya answers only after becoming part of the world."
    );

    let loaded = observer.load().await.expect("load generated save");
    let generated = loaded
        .records
        .iter()
        .find_map(|record| match record {
            DomainRecord::Character(character) if character.display_name.as_str() == "Ilya" => {
                Some(character)
            }
            _ => None,
        })
        .expect("generated character is durable");
    assert_eq!(
        generated.lifetime,
        CharacterLifetime::Scene {
            scene_id: fixture.scene
        }
    );
    let EntityOrigin::Generated { origin } = &generated.origin else {
        panic!("generated provenance")
    };
    assert!(matches!(
        origin.source,
        GenerationSource::PlayerInput { transcript_id }
            if transcript_id == loaded.transcripts[0].id
    ));
    GameWorld::from_records(
        loaded.revision,
        loaded.records,
        fixture.world_config,
        &fixture.registry,
    )
    .expect("generated save rebuilds without a provider");
}

#[tokio::test]
async fn repeated_materialization_request_does_not_generate_a_second_character() {
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
        fixture.registry,
        &fixture.manifest.world_lock,
        &fixture.manifest.mod_lock,
        fixture.world_config,
    )
    .await
    .expect("open service");
    let policy_id = definition_id("generation_policy", "deduplicated");
    let narrator = Arc::new(GeneratedNarratorBridge::repeating(
        fixture.profile_id.clone(),
    ));
    let npc = Arc::new(MockBridge::scripted([MockResponse::text(
        "There is still only one of me.",
    )]));
    let config = RuntimeConfig {
        generation_policy: Some(generation_policy(policy_id, fixture.profile_id)),
        ..RuntimeConfig::default()
    };
    let mut runtime = GameRuntime::new(
        service,
        narrator.clone(),
        narrator_definition(),
        parse("ses_01890f6a-2ba7-7d4e-8f90-123456789abc"),
        config,
    );
    runtime.set_default_npc_bridge(npc.clone());

    runtime
        .handle_player_input("Find the same witness twice, then ask once.")
        .await
        .expect("complete deduplicated NPC turn");

    assert_eq!(narrator.calls.load(Ordering::SeqCst), 8);
    assert_eq!(npc.requests().expect("NPC requests").len(), 1);
    let loaded = observer.load().await.expect("load deduplicated save");
    assert_eq!(
        loaded
            .records
            .iter()
            .filter(|record| matches!(
                record,
                DomainRecord::Character(CharacterRecord {
                    origin: EntityOrigin::Generated { .. },
                    ..
                })
            ))
            .count(),
        1
    );
}

#[tokio::test]
async fn preset_npc_uses_the_same_spawn_event_and_post_commit_replanning_barrier() {
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
        &fixture.manifest.world_lock,
        &fixture.manifest.mod_lock,
        fixture.world_config.clone(),
    )
    .await
    .expect("open service");
    let narrator = Arc::new(PresetNarratorBridge::new(fixture.preset_id.clone()));
    let npc = Arc::new(MockBridge::scripted([MockResponse::text(
        "I was already expected.",
    )]));
    let mut runtime = GameRuntime::new(
        Arc::clone(&service),
        narrator.clone(),
        narrator_definition(),
        parse("ses_01890f6a-2ba3-7d4e-8f90-123456789abc"),
        RuntimeConfig::default(),
    );
    runtime.set_default_npc_bridge(npc.clone());

    let outcome = runtime
        .handle_player_input("Invite the expected witness into the conversation.")
        .await
        .expect("complete preset NPC turn");

    assert!(narrator.saw_materialized_profile.load(Ordering::SeqCst));
    assert_eq!(narrator.calls.load(Ordering::SeqCst), 5);
    assert_eq!(npc.requests().expect("preset NPC requests").len(), 1);
    assert_eq!(outcome.npc_results.len(), 1);
    assert_eq!(
        outcome.npc_results[0].observed_revision,
        Some(Revision::new(2))
    );
    assert_eq!(outcome.snapshot.revision, Revision::new(3));

    let loaded = observer.load().await.expect("load preset save");
    let preset = loaded
        .records
        .iter()
        .find_map(|record| match record {
            DomainRecord::Character(character) if character.display_name.as_str() == "Orin" => {
                Some(character)
            }
            _ => None,
        })
        .expect("preset character is durable");
    assert_eq!(preset.lifetime, CharacterLifetime::Persistent);
    assert!(matches!(preset.origin, EntityOrigin::Content { .. }));
    assert!(service.events().await.iter().any(|event| {
        matches!(
            event.kind,
            loreloom_core::WorldEventKind::CharacterSpawned { character_id }
                if character_id == preset.id
        )
    }));
}

#[tokio::test]
async fn generation_limits_and_provider_failures_return_to_narrator_without_partial_npcs() {
    for (index, mode) in [
        GenerationRejectionMode::ResourceLimit,
        GenerationRejectionMode::ProviderFailure,
    ]
    .into_iter()
    .enumerate()
    {
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
            &fixture.manifest.world_lock,
            &fixture.manifest.mod_lock,
            fixture.world_config.clone(),
        )
        .await
        .expect("open service");
        let policy_id = definition_id("generation_policy", "rejected");
        let narrator = Arc::new(RejectedGenerationBridge::new(mode));
        let npc_resources = if matches!(mode, GenerationRejectionMode::ResourceLimit) {
            NpcResourcePolicy {
                max_generated_per_orchestration: 0,
                ..NpcResourcePolicy::default()
            }
        } else {
            NpcResourcePolicy::default()
        };
        let config = RuntimeConfig {
            npc_resources,
            generation_policy: Some(generation_policy(policy_id, fixture.profile_id)),
            ..RuntimeConfig::default()
        };
        let mut runtime = GameRuntime::new(
            service,
            narrator.clone(),
            narrator_definition(),
            parse(&format!(
                "ses_01890f6a-2ba{}-7d4e-8f90-123456789abc",
                index + 4
            )),
            config,
        );

        let outcome = runtime
            .handle_player_input("Find a witness, or continue without one.")
            .await
            .expect("narrator recovers from rejected generation");

        assert!(narrator.saw_rejection.load(Ordering::SeqCst));
        assert!(outcome.npc_results.is_empty());
        assert_eq!(outcome.snapshot.revision, Revision::new(2));
        if matches!(mode, GenerationRejectionMode::ProviderFailure) {
            let diagnostic = outcome
                .snapshot
                .notices
                .iter()
                .find(|notice| notice.message.as_str().contains("provider_rejected"))
                .expect("generation failure warning");
            assert!(diagnostic.message.as_str().contains("HTTP 422"));
            assert!(diagnostic.message.as_str().contains("ref err_"));
            assert!(!diagnostic.message.as_str().contains("must-not-escape"));
        }
        let loaded = observer
            .load()
            .await
            .expect("load rejected generation save");
        assert!(!loaded.records.iter().any(|record| {
            matches!(
                record,
                DomainRecord::Character(CharacterRecord {
                    origin: EntityOrigin::Generated { .. },
                    ..
                })
            )
        }));
    }
}

#[tokio::test]
async fn cancellation_during_npc_generation_does_not_publish_a_character() {
    let directory = TempDir::new().expect("temporary save parent");
    let fixture = fixture();
    let store = SaveStore::create(
        directory.path().join("save"),
        fixture.manifest.clone(),
        fixture.records,
    )
    .await
    .expect("create save");
    let mut observer = store.connect().await.expect("observer connection");
    let service = WorldService::open(
        store,
        fixture.registry,
        &fixture.manifest.world_lock,
        &fixture.manifest.mod_lock,
        fixture.world_config,
    )
    .await
    .expect("open service");
    let policy_id = definition_id("generation_policy", "cancelled");
    let policy = generation_policy(policy_id.clone(), fixture.profile_id.clone());
    let entered_generation = Arc::new(tokio::sync::Notify::new());
    let narrator = Arc::new(CancellableGenerationBridge {
        request: CreateNpcRequest {
            source: NpcCreationSource::Generated {
                role: text("delayed witness"),
                purpose: LongText::new("Wait until cancelled.").expect("purpose"),
            },
            lifetime: NpcLifetime::Scene,
            mode: NpcCreationMode::Agent,
        },
        calls: AtomicUsize::new(0),
        entered_generation: Arc::clone(&entered_generation),
    });
    let config = RuntimeConfig {
        generation_policy: Some(policy),
        ..RuntimeConfig::default()
    };
    let mut runtime = GameRuntime::new(
        service,
        narrator,
        narrator_definition(),
        parse("ses_01890f6a-2ba6-7d4e-8f90-123456789abc"),
        config,
    );
    let cancellation = runtime.cancellation_token();
    let task = tokio::spawn(async move {
        runtime
            .handle_player_input("Wait for a witness who may never arrive.")
            .await
    });

    entered_generation.notified().await;
    cancellation.cancel();
    let error = task
        .await
        .expect("runtime task")
        .expect_err("generation is cancelled");
    assert!(matches!(error, RuntimeError::Cancelled));

    let loaded = observer.load().await.expect("load after cancellation");
    assert_eq!(loaded.revision, Revision::new(1));
    assert!(!loaded.records.iter().any(|record| {
        matches!(
            record,
            DomainRecord::Character(CharacterRecord {
                origin: EntityOrigin::Generated { .. },
                ..
            })
        )
    }));
}

#[tokio::test]
async fn npc_generation_consumes_the_shared_started_agent_turn_budget() {
    let directory = TempDir::new().expect("temporary save parent");
    let fixture = fixture();
    let store = SaveStore::create(
        directory.path().join("save"),
        fixture.manifest.clone(),
        fixture.records,
    )
    .await
    .expect("create save");
    let mut observer = store.connect().await.expect("observer connection");
    let service = WorldService::open(
        store,
        fixture.registry,
        &fixture.manifest.world_lock,
        &fixture.manifest.mod_lock,
        fixture.world_config,
    )
    .await
    .expect("open service");
    let policy_id = definition_id("generation_policy", "turn_budget");
    let narrator = Arc::new(GeneratedNarratorBridge::new(fixture.profile_id.clone()));
    let config = RuntimeConfig {
        orchestration_budget: OrchestrationBudget {
            max_started_agent_turns: 1,
            ..OrchestrationBudget::default()
        },
        generation_policy: Some(generation_policy(policy_id, fixture.profile_id)),
        ..RuntimeConfig::default()
    };
    let mut runtime = GameRuntime::new(
        service,
        narrator.clone(),
        narrator_definition(),
        parse("ses_01890f6a-2ba8-7d4e-8f90-123456789abc"),
        config,
    );

    let error = runtime
        .handle_player_input("Generate beyond this orchestration's turn budget.")
        .await
        .expect_err("generation cannot start outside the shared budget");

    assert!(matches!(
        error,
        RuntimeError::Budget(loreloom_agent::BudgetReason::AgentTurns)
    ));
    assert_eq!(narrator.calls.load(Ordering::SeqCst), 2);
    let loaded = observer.load().await.expect("load budget-limited save");
    assert!(!loaded.records.iter().any(|record| {
        matches!(
            record,
            DomainRecord::Character(CharacterRecord {
                origin: EntityOrigin::Generated { .. },
                ..
            })
        )
    }));
}

#[tokio::test]
async fn request_npc_turn_derives_the_current_scene_instead_of_accepting_model_scene_state() {
    let directory = TempDir::new().expect("temporary save parent");
    let fixture = fixture();
    let candidate_world_lock = fixture.manifest.world_lock.clone();
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
        &candidate_world_lock,
        &candidate_mod_lock,
        fixture.world_config,
    )
    .await
    .expect("open service");
    let executor = RuntimeToolExecutor::new(Arc::clone(&service));
    let rejected_legacy_shape = executor
        .execute(
            ToolContext::new().with_extension(AgentToolContext {
                actor_id: fixture.player,
                revision: Revision::ZERO,
                session_id: parse("ses_01890f6a-2bc1-7d4e-8f90-123456789abc"),
                capabilities: BTreeSet::from(["narrator.request_npc_turn".to_owned()]),
            }),
            ToolCall {
                id: ToolCallId::new("request-with-model-scene").expect("tool call ID"),
                name: "request_npc_turn".to_owned(),
                arguments: json!({
                    "actor_id": fixture.npc,
                    "scene_id": fixture.scene,
                    "assignment": "The runtime must reject model-supplied scene state."
                }),
            },
        )
        .await
        .expect("correlated legacy-shape rejection");
    assert!(rejected_legacy_shape.is_error);
    assert_eq!(
        tool_result_json(&rejected_legacy_shape)["code"],
        json!("invalid_input")
    );
    let plan = NarratorPlan {
        based_on_revision: Revision::new(1),
        npc_turns: vec![request(fixture.npc, object_id("2b6f"))],
    };
    let npc = npc_bridge();
    let mut runtime = GameRuntime::new(
        service,
        Arc::new(NarratorBridge::new(plan, SupportMode::Empty)),
        narrator_definition(),
        parse("ses_01890f6a-2b62-7d4e-8f90-123456789abc"),
        RuntimeConfig::default(),
    );
    runtime.register_npc(fixture.npc, definition(fixture.profile_id), npc.clone());

    let outcome = runtime
        .handle_player_input("Ask Mira to answer in the current scene.")
        .await
        .expect("complete with runtime-derived scene");

    assert_eq!(outcome.npc_results.len(), 1);
    assert_eq!(outcome.npc_results[0].status, NpcTurnStatus::Completed);
    assert!(outcome.npc_results[0].observed_revision.is_some());
    assert_eq!(npc.requests().expect("npc request log").len(), 2);
}

#[tokio::test]
async fn narrator_text_cannot_supply_world_event_provenance() {
    let directory = TempDir::new().expect("temporary save parent");
    let fixture = fixture();
    let candidate_world_lock = fixture.manifest.world_lock.clone();
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
        &candidate_world_lock,
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
        narrator_definition(),
        parse("ses_01890f6a-2b63-7d4e-8f90-123456789abc"),
        RuntimeConfig::default(),
    );

    let outcome = runtime
        .handle_player_input("Say that an impossible event occurred.")
        .await
        .expect("natural-language narration completes");

    assert!(outcome.snapshot.supporting_events.is_empty());
    let loaded = observer.load().await.expect("load completed narration");
    assert_eq!(loaded.revision, Revision::new(2));
    assert_eq!(loaded.transcripts.len(), 2);
    assert!(matches!(
        loaded.transcripts[0].speaker,
        TranscriptSpeaker::Player { .. }
    ));
    assert!(loaded.transcripts[1].supporting_events.is_empty());
}

#[tokio::test]
async fn narrator_response_body_is_never_parsed_as_a_control_envelope() {
    let directory = TempDir::new().expect("temporary save parent");
    let fixture = fixture();
    let candidate_world_lock = fixture.manifest.world_lock.clone();
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
        &candidate_world_lock,
        &candidate_mod_lock,
        fixture.world_config,
    )
    .await
    .expect("open service");
    let literal = r#"{"kind":"continue","next_plan":{"npc_turns":[]}}"#;
    let narrator = Arc::new(MockBridge::scripted([MockResponse::text(literal)]));
    let mut runtime = GameRuntime::new(
        service,
        narrator,
        narrator_definition(),
        parse("ses_01890f6a-2bc0-7d4e-8f90-123456789abc"),
        RuntimeConfig::default(),
    );

    let outcome = runtime
        .handle_player_input("Tell me what happens next.")
        .await
        .expect("literal model text is valid narration");

    assert_eq!(outcome.narration.as_str(), literal);
    assert!(outcome.npc_results.is_empty());
}

#[tokio::test]
async fn narrator_and_npc_contexts_apply_host_projection_limits_before_provider_calls() {
    let directory = TempDir::new().expect("temporary save parent");
    let mut fixture = fixture();
    let _ = add_tool_records(&mut fixture);
    fixture.records.extend([
        DomainRecord::KnownFact(KnownFactRecord {
            id: object_id("2bb9"),
            owner_id: fixture.npc,
            subject: FactSubject::World,
            predicate_id: ContentDefinitionId::parse(DIAGNOSED_CONDITION_PREDICATE_ID)
                .expect("fact predicate"),
            value: FactValue::Bool(true),
            status: KnowledgeStatus::Confirmed,
            confidence: Fixed::ONE,
            source: FactSource::Content {
                definition_id: fixture.preset_id.clone(),
            },
            first_known_at: WorldTime::ZERO,
            last_confirmed_at: WorldTime::ZERO,
        }),
        DomainRecord::Goal(GoalRecord {
            id: object_id("2bba"),
            owner_id: fixture.npc,
            description: text("Keep the inn quiet."),
            priority: 1,
            status: GoalStatus::Active,
            source: GoalSource::CharacterDefinition {
                definition_id: fixture.preset_id.clone(),
            },
            updated_at: WorldTime::ZERO,
        }),
    ]);
    let candidate_world_lock = fixture.manifest.world_lock.clone();
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
        &candidate_world_lock,
        &candidate_mod_lock,
        fixture.world_config,
    )
    .await
    .expect("open service");
    let narrator = Arc::new(NarratorBridge::new(
        NarratorPlan {
            based_on_revision: Revision::new(1),
            npc_turns: vec![request(fixture.npc, fixture.scene)],
        },
        SupportMode::Empty,
    ));
    let npc = npc_bridge();
    let mut runtime = GameRuntime::new(
        service,
        narrator.clone(),
        narrator_definition(),
        parse("ses_01890f6a-2bbe-7d4e-8f90-123456789abc"),
        RuntimeConfig {
            context_projection: ContextProjectionPolicy {
                transcript_items: 0,
                transcript_bytes: 0,
                known_facts: 0,
                goals: 0,
                visible_actors: 2,
                inventory_items: 0,
                skills: 0,
                max_context_tokens: 131_072,
            },
            ..RuntimeConfig::default()
        },
    );
    runtime.register_npc(fixture.npc, definition(fixture.profile_id), npc.clone());

    runtime
        .handle_player_input("Ask Mira to answer without exposing unrelated state.")
        .await
        .expect("bounded turn");

    let observation = narrator.planning_observation();
    assert_eq!(observation["truncated"], json!(true));
    assert!(
        observation["player"]["inventory"]
            .as_array()
            .expect("inventory")
            .is_empty()
    );
    assert!(
        observation["player"]["skills"]
            .as_array()
            .expect("skills")
            .is_empty()
    );
    assert_eq!(
        observation["scene"]["visible_actors"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    assert!(
        observation["recent_transcript"]
            .as_array()
            .expect("transcript")
            .is_empty()
    );

    let npc_requests = npc.requests().expect("NPC request log");
    let npc_context = npc_requests[0]
        .messages
        .iter()
        .flat_map(|message| message.content.iter())
        .find_map(|part| match part {
            armillae_core::ContentPart::Text(text) => {
                serde_json::from_str::<JsonValue>(&text.text).ok()
            }
            _ => None,
        })
        .expect("NPC context envelope");
    assert_eq!(npc_context["context"]["truncated"], json!(true));
    assert!(
        npc_context["context"]["character"]["inventory"]
            .as_array()
            .expect("NPC inventory")
            .is_empty()
    );
    assert!(
        npc_context["context"]["character"]["known_facts"]
            .as_array()
            .expect("NPC known facts")
            .is_empty()
    );
    assert!(
        npc_context["context"]["character"]["goals"]
            .as_array()
            .expect("NPC goals")
            .is_empty()
    );
    assert!(
        npc_context["context"]["recent_dialogue"]
            .as_array()
            .expect("NPC dialogue")
            .is_empty()
    );
}

#[tokio::test]
async fn inventory_tools_page_by_stable_id_enforce_actor_ownership_and_update_snapshot() {
    let directory = TempDir::new().expect("temporary save parent");
    let mut fixture = fixture();
    let ids = add_tool_records(&mut fixture);
    let player_root = fixture
        .records
        .iter()
        .find_map(|record| match record {
            DomainRecord::Character(character) if character.id == fixture.player => {
                Some(character.inventory_root)
            }
            _ => None,
        })
        .expect("player inventory root");
    let npc_root = fixture
        .records
        .iter()
        .find_map(|record| match record {
            DomainRecord::Character(character) if character.id == fixture.npc => {
                Some(character.inventory_root)
            }
            _ => None,
        })
        .expect("npc inventory root");
    let store = SaveStore::create(
        directory.path().join("save"),
        fixture.manifest.clone(),
        fixture.records,
    )
    .await
    .expect("create save");
    let service = WorldService::open(
        store,
        fixture.registry,
        &fixture.manifest.world_lock,
        &fixture.manifest.mod_lock,
        fixture.world_config,
    )
    .await
    .expect("open service");
    let executor = RuntimeToolExecutor::new(Arc::clone(&service));
    let session_id = parse("ses_01890f6a-2bc3-7d4e-8f90-123456789abc");
    let context = |revision| {
        ToolContext::new().with_extension(AgentToolContext {
            actor_id: fixture.player,
            revision,
            session_id,
            capabilities: Default::default(),
        })
    };

    let first = executor
        .execute(
            context(Revision::ZERO),
            ToolCall {
                id: ToolCallId::new("inventory-first-page").expect("call id"),
                name: "list_inventory".to_owned(),
                arguments: json!({ "limit": 1 }),
            },
        )
        .await
        .expect("correlated first page");
    let ToolResultContent::Json { value: first } = &first.content[0] else {
        panic!("JSON first page")
    };
    assert_eq!(first["items"].as_array().expect("items").len(), 1);
    let cursor = first["next_after"]
        .as_str()
        .expect("next cursor")
        .to_owned();
    let rest = executor
        .execute(
            context(Revision::ZERO),
            ToolCall {
                id: ToolCallId::new("inventory-rest").expect("call id"),
                name: "list_inventory".to_owned(),
                arguments: json!({ "after": cursor, "limit": 64 }),
            },
        )
        .await
        .expect("correlated remaining page");
    let ToolResultContent::Json { value: rest } = &rest.content[0] else {
        panic!("JSON remaining page")
    };
    assert!(
        rest["items"]
            .as_array()
            .expect("remaining items")
            .iter()
            .all(|item| item["item_id"] != json!(npc_root))
    );

    let denied = executor
        .execute(
            context(Revision::ZERO),
            ToolCall {
                id: ToolCallId::new("inspect-foreign-item").expect("call id"),
                name: "inspect_item".to_owned(),
                arguments: json!({ "item_id": npc_root }),
            },
        )
        .await
        .expect("correlated ownership rejection");
    assert!(denied.is_error);
    assert!(matches!(
        &denied.content[..],
        [ToolResultContent::Json { value }] if value["code"] == json!("unavailable")
    ));
    let inspected_item = executor
        .execute(
            context(Revision::ZERO),
            ToolCall {
                id: ToolCallId::new("inspect-owned-item").expect("call id"),
                name: "inspect_item".to_owned(),
                arguments: json!({ "item_id": ids.stack }),
            },
        )
        .await
        .expect("correlated item inspection");
    assert!(!inspected_item.is_error);

    let skills = executor
        .execute(
            context(Revision::ZERO),
            ToolCall {
                id: ToolCallId::new("list-skills").expect("call id"),
                name: "list_available_skills".to_owned(),
                arguments: json!({}),
            },
        )
        .await
        .expect("correlated skill list");
    let ToolResultContent::Json { value: skills } = &skills.content[0] else {
        panic!("JSON skill list")
    };
    assert_eq!(skills["skills"].as_array().expect("skills").len(), 1);
    assert_eq!(skills["skills"][0]["grant_id"], json!(ids.grant));
    assert_eq!(skills["skills"][0]["available"], json!(true));
    let inspected = executor
        .execute(
            context(Revision::ZERO),
            ToolCall {
                id: ToolCallId::new("inspect-skill").expect("call id"),
                name: "inspect_skill".to_owned(),
                arguments: json!({ "grant_id": ids.grant }),
            },
        )
        .await
        .expect("correlated skill inspection");
    assert!(!inspected.is_error);
    let denied_skill = executor
        .execute(
            context(Revision::ZERO),
            ToolCall {
                id: ToolCallId::new("inspect-foreign-skill").expect("call id"),
                name: "inspect_skill".to_owned(),
                arguments: json!({ "grant_id": ids.npc_grant }),
            },
        )
        .await
        .expect("correlated skill ownership rejection");
    assert!(denied_skill.is_error);

    let transferred = executor
        .execute(
            context(Revision::ZERO),
            ToolCall {
                id: ToolCallId::new("transfer-stack").expect("call id"),
                name: "transfer_item".to_owned(),
                arguments: json!({ "item_id": ids.stack, "container_id": player_root }),
            },
        )
        .await
        .expect("correlated transfer");
    assert!(!transferred.is_error);
    let equipped = executor
        .execute(
            context(Revision::new(1)),
            ToolCall {
                id: ToolCallId::new("equip-gear").expect("call id"),
                name: "equip_item".to_owned(),
                arguments: json!({
                    "item_id": ids.gear,
                    "slot_id": definition_id("equipment_slot", "hand")
                }),
            },
        )
        .await
        .expect("correlated equip");
    assert!(!equipped.is_error);

    let stale = executor
        .execute(
            context(Revision::ZERO),
            ToolCall {
                id: ToolCallId::new("stale-inventory").expect("call id"),
                name: "list_inventory".to_owned(),
                arguments: json!({}),
            },
        )
        .await
        .expect("correlated stale result");
    assert!(stale.is_error);
    let snapshot = service
        .snapshot(
            session_id,
            loreloom_core::RuntimePhase::Idle,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .await
        .expect("updated snapshot");
    assert_eq!(snapshot.revision, Revision::new(2));
    assert_eq!(
        snapshot
            .player
            .inventory
            .iter()
            .find(|item| item.item.id == ids.stack)
            .expect("transferred stack")
            .item
            .contained_by,
        Some(player_root)
    );
    assert_eq!(
        snapshot
            .player
            .inventory
            .iter()
            .find(|item| item.item.id == ids.gear)
            .expect("equipped gear")
            .item
            .equipped
            .as_ref()
            .expect("equipment state")
            .slot_id,
        definition_id("equipment_slot", "hand")
    );
}

#[tokio::test]
async fn mock_agent_continuation_splits_stack_then_uses_skill_at_advanced_revision() {
    let directory = TempDir::new().expect("temporary save parent");
    let mut fixture = fixture();
    let ids = add_tool_records(&mut fixture);
    let store = SaveStore::create(
        directory.path().join("save"),
        fixture.manifest.clone(),
        fixture.records,
    )
    .await
    .expect("create save");
    let service = WorldService::open(
        store,
        fixture.registry,
        &fixture.manifest.world_lock,
        &fixture.manifest.mod_lock,
        fixture.world_config,
    )
    .await
    .expect("open service");
    let executor = Arc::new(RuntimeToolExecutor::new(Arc::clone(&service)));
    let tools = executor
        .definitions()
        .into_iter()
        .filter(|definition| matches!(definition.name.as_str(), "split_stack" | "use_skill"))
        .collect();
    let runner = AgentRunner::new(executor);
    let bridge = MockBridge::scripted([
        MockResponse::tool_call(
            ToolCallId::new("split-from-agent").expect("call id"),
            "split_stack",
            json!({ "item_id": ids.stack, "quantity": 1 }),
        ),
        MockResponse::tool_call(
            ToolCallId::new("skill-from-agent").expect("call id"),
            "use_skill",
            json!({
                "grant_id": ids.grant,
                "target": { "type": "self_target" }
            }),
        ),
        MockResponse::text("done"),
    ]);
    let session_id = parse("ses_01890f6a-2bc4-7d4e-8f90-123456789abc");
    let cancellation = CancellationToken::new();
    let outcome = runner
        .run_turn(TurnInvocation {
            model_invocation: loreloom_agent::ModelInvocationKind::Npc,
            bridge: &bridge,
            request: CompletionRequest {
                tools,
                ..CompletionRequest::default()
            },
            tool_context: AgentToolContext {
                actor_id: fixture.player,
                revision: Revision::ZERO,
                session_id,
                capabilities: BTreeSet::from(["split_stack".to_owned(), "use_skill".to_owned()]),
            },
            budget: ResourceBudget::default(),
            max_context_tokens: u64::MAX,
            cancellation: &cancellation,
        })
        .await;
    assert_eq!(outcome.status, TurnStatus::Completed);
    assert!(outcome.tool_calls.iter().all(|call| !call.is_error));
    assert_eq!(outcome.committed_events.len(), 3);
    let events = service.events().await;
    assert!(
        events
            .iter()
            .any(|event| matches!(event.kind, WorldEventKind::StackSplit { .. }))
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event.kind, WorldEventKind::ResourceChanged { .. }))
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event.kind, WorldEventKind::SkillUsed { .. }))
    );
    let snapshot = service
        .snapshot(
            session_id,
            loreloom_core::RuntimePhase::Idle,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .await
        .expect("updated snapshot");
    assert_eq!(snapshot.revision, Revision::new(2));
    assert_eq!(
        snapshot
            .player
            .inventory
            .iter()
            .find(|item| item.item.id == ids.stack)
            .expect("source stack")
            .item
            .stack
            .0
            .get(),
        2
    );
    assert_eq!(
        snapshot
            .player
            .resources
            .iter()
            .find(|resource| resource.resource_id == definition_id("resource", "focus"))
            .expect("focus resource")
            .current,
        Fixed::from_integer(4).expect("remaining focus")
    );
    assert_eq!(
        snapshot
            .player
            .skills
            .iter()
            .find(|skill| skill.grant.id == ids.grant)
            .expect("skill grant")
            .grant
            .ready_at,
        Some(WorldTime::from_ticks(3))
    );
}

#[tokio::test]
async fn external_revision_conflict_recovers_the_candidate_world() {
    let directory = TempDir::new().expect("temporary save parent");
    let fixture = fixture();
    let candidate_world_lock = fixture.manifest.world_lock.clone();
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
        &candidate_world_lock,
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
        &fixture.manifest.world_lock,
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
