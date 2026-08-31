use std::{collections::BTreeMap, num::NonZeroU32};

use loreloom_content::{
    AgentProfileDefinition, CONTENT_SCHEMA_V1, CharacterDefinition, ContainerDefinition,
    ContentDocument, ContentPackContext, Definition, DefinitionRegistry, EquipmentSlotDefinition,
    InitialCharacterController, InitialCharacterLifetime, InitialItem, InitialResource,
    InitialSkill, ItemDefinition, ResourceCost, ResourceDefinition, ResourceMaximumPolicy,
    SceneCharacterDefinition, SceneDefinition, SkillDefinition, SkillKind, SkillTarget,
    parse_content_hash,
};
use loreloom_core::{
    ActionId, ActionState, ActorId, BaseAttributes, CharacterController, CharacterLifetime,
    CharacterProfile, CharacterRecord, CharacterSpawnSpec, ContentDefinitionId, ContentOrigin,
    DisplayName, DomainRecord, EntityOrigin, Fixed, ItemRecord, LifeState, ModId, ObjectId,
    PlaceRecord, PlacementInput, Posture, ResourcePool, Revision, SceneRecord,
    SceneTransitionTarget, ShortText, SkillGrantRecord, SkillSource, SkillTargetRef,
    SpawnConstraints, StackState, SystemIdGenerator, TranscriptItemId, TranscriptItemRecord,
    TranscriptSpeaker, TranscriptState, WorldCommand, WorldCommandKind, WorldEventKind, WorldId,
    WorldStateRecord, WorldTime,
};
use loreloom_world::{GameWorld, WorldConfig, WorldError};
use semver::Version;

fn definition_id(kind: &str, key: &str) -> ContentDefinitionId {
    format!("games.loreloom.worldtest:{kind}/{key}")
        .parse()
        .expect("definition id")
}

fn object_id(suffix: &str) -> ObjectId {
    format!("obj_01890f6a-{suffix}-7d4e-8f90-123456789abc")
        .parse()
        .expect("object id")
}

fn action_id(suffix: &str) -> ActionId {
    format!("act_01890f6a-{suffix}-7d4e-8f90-123456789abc")
        .parse()
        .expect("action id")
}

fn text(value: &str) -> ShortText {
    ShortText::new(value).expect("short text")
}

fn name(value: &str) -> DisplayName {
    DisplayName::new(value).expect("display name")
}

struct Fixture {
    registry: DefinitionRegistry,
    records: Vec<DomainRecord>,
    config: WorldConfig,
    player: ActorId,
    scene: ObjectId,
    forest_scene_definition: ContentDefinitionId,
    quay: ObjectId,
    inn: ObjectId,
    root: ObjectId,
    coin: ObjectId,
    grant: ObjectId,
}

fn fixture() -> Fixture {
    let mod_id = ModId::parse("games.loreloom.worldtest").expect("mod id");
    let pack_id = definition_id("pack", "worldtest");
    let resource_id = definition_id("resource", "stamina");
    let root_definition = definition_id("item", "inventory_root");
    let coin_definition = definition_id("item", "coin");
    let slot_id = definition_id("equipment_slot", "hand");
    let skill_id = definition_id("skill", "focus");
    let quay_definition = definition_id("place", "quay");
    let inn_definition = definition_id("place", "inn");
    let scene_definition = definition_id("scene", "harbor");
    let forest_place_definition = definition_id("place", "forest_path");
    let forest_scene_definition = definition_id("scene", "forest");
    let player_definition = definition_id("character", "traveler");
    let agent_definition = definition_id("character", "warden");
    let agent_profile = definition_id("agent_profile", "warden");
    let document = ContentDocument {
        schema_version: CONTENT_SCHEMA_V1,
        definitions: vec![
            Definition::Resource(ResourceDefinition {
                id: resource_id.clone(),
                display_name: name("Stamina"),
                minimum: Fixed::ZERO,
                maximum: Fixed::from_integer(100).expect("fixed"),
                maximum_policy: ResourceMaximumPolicy::ClampCurrent,
                derived_from_attribute: None,
            }),
            Definition::EquipmentSlot(EquipmentSlotDefinition {
                id: slot_id.clone(),
                display_name: name("Hand"),
                allowed_item_tags: Default::default(),
            }),
            Definition::Item(ItemDefinition {
                id: root_definition.clone(),
                display_name: name("Inventory"),
                description: text("Inventory root."),
                tags: Default::default(),
                stack_limit: NonZeroU32::MIN,
                unit_weight_grams: Fixed::ZERO,
                durability: None,
                container: Some(ContainerDefinition {
                    max_weight_grams: Fixed::from_integer(10_000).expect("fixed"),
                    max_children: 32,
                }),
                equipment_slots: Default::default(),
                modifiers: Vec::new(),
            }),
            Definition::Item(ItemDefinition {
                id: coin_definition.clone(),
                display_name: name("Coin"),
                description: text("A brass coin."),
                tags: Default::default(),
                stack_limit: NonZeroU32::new(100).expect("non-zero"),
                unit_weight_grams: Fixed::from_integer(5).expect("fixed"),
                durability: None,
                container: None,
                equipment_slots: std::collections::BTreeSet::from([slot_id]),
                modifiers: Vec::new(),
            }),
            Definition::Skill(SkillDefinition {
                id: skill_id.clone(),
                display_name: name("Focus"),
                description: text("Spend stamina to focus."),
                kind: SkillKind::Active,
                costs: vec![ResourceCost {
                    resource_id: resource_id.clone(),
                    amount: Fixed::ONE,
                }],
                cooldown_ticks: 3,
                target: SkillTarget::SelfTarget,
                executor_id: definition_id("skill_executor", "effects"),
                effects: Vec::new(),
                reaction: None,
            }),
            Definition::Place(loreloom_content::PlaceDefinition {
                id: quay_definition.clone(),
                display_name: name("Quay"),
                description: text("Wet stone."),
                tags: Default::default(),
                edges: std::collections::BTreeSet::from([inn_definition.clone()]),
            }),
            Definition::Place(loreloom_content::PlaceDefinition {
                id: inn_definition.clone(),
                display_name: name("Inn"),
                description: text("A warm common room."),
                tags: Default::default(),
                edges: std::collections::BTreeSet::from([quay_definition.clone()]),
            }),
            Definition::Place(loreloom_content::PlaceDefinition {
                id: forest_place_definition.clone(),
                display_name: name("Forest Path"),
                description: text("Pines close around an old path."),
                tags: Default::default(),
                edges: Default::default(),
            }),
            Definition::Scene(SceneDefinition {
                id: scene_definition.clone(),
                display_name: name("Harbor"),
                framing: text("Rain falls over the harbor."),
                entry_place: quay_definition.clone(),
                places: std::collections::BTreeSet::from([
                    quay_definition.clone(),
                    inn_definition.clone(),
                ]),
                characters: vec![
                    SceneCharacterDefinition {
                        local_key: text("player"),
                        character_id: player_definition.clone(),
                        place_id: quay_definition.clone(),
                        controller: InitialCharacterController::Player,
                        lifetime: InitialCharacterLifetime::Persistent,
                    },
                    SceneCharacterDefinition {
                        local_key: text("warden"),
                        character_id: agent_definition.clone(),
                        place_id: inn_definition.clone(),
                        controller: InitialCharacterController::Agent,
                        lifetime: InitialCharacterLifetime::Scene,
                    },
                ],
            }),
            Definition::Scene(SceneDefinition {
                id: forest_scene_definition.clone(),
                display_name: name("Forest"),
                framing: text("Mist gathers beneath the pines."),
                entry_place: forest_place_definition.clone(),
                places: std::collections::BTreeSet::from([forest_place_definition.clone()]),
                characters: vec![
                    SceneCharacterDefinition {
                        local_key: text("player"),
                        character_id: player_definition.clone(),
                        place_id: forest_place_definition.clone(),
                        controller: InitialCharacterController::Player,
                        lifetime: InitialCharacterLifetime::Persistent,
                    },
                    SceneCharacterDefinition {
                        local_key: text("warden"),
                        character_id: agent_definition.clone(),
                        place_id: forest_place_definition,
                        controller: InitialCharacterController::Agent,
                        lifetime: InitialCharacterLifetime::Scene,
                    },
                ],
            }),
            Definition::AgentProfile(AgentProfileDefinition {
                id: agent_profile.clone(),
                display_name: name("Warden"),
                system_style: text("Guard the harbor."),
                model_alias: text("test"),
                tool_capabilities: Default::default(),
                autonomy: loreloom_core::AutonomyMode::Directed,
            }),
            Definition::Character(CharacterDefinition {
                id: player_definition,
                display_name: name("Traveler"),
                profile: CharacterProfile {
                    summary: text("A newly arrived traveler."),
                    values: Vec::new(),
                    speaking_style: text("Direct."),
                    narrative_tags: Default::default(),
                },
                agent_profile: None,
                base_attributes: BaseAttributes::default(),
                resources: vec![InitialResource {
                    resource_id: resource_id.clone(),
                    current: Fixed::from_integer(10).expect("fixed"),
                    base_maximum: Fixed::from_integer(10).expect("fixed"),
                }],
                conditions: Vec::new(),
                inventory: vec![InitialItem {
                    local_key: text("coin"),
                    item_id: coin_definition.clone(),
                    quantity: NonZeroU32::new(3).expect("non-zero"),
                    parent_local_key: None,
                }],
                skills: vec![InitialSkill {
                    skill_id: skill_id.clone(),
                    rank: NonZeroU32::MIN,
                    proficiency: 0,
                    enabled: true,
                }],
                knowledge: Vec::new(),
                goals: Vec::new(),
                spawn_constraints: SpawnConstraints {
                    minimum_attributes: BTreeMap::new(),
                    maximum_attributes: BTreeMap::new(),
                    maximum_attribute_points: Fixed::ZERO,
                    maximum_items: 1,
                    maximum_skills: 1,
                    allowed_definitions: std::collections::BTreeSet::from([
                        coin_definition.clone(),
                        skill_id.clone(),
                    ]),
                },
            }),
            Definition::Character(CharacterDefinition {
                id: agent_definition,
                display_name: name("Harbor Warden"),
                profile: CharacterProfile {
                    summary: text("A watchful harbor warden."),
                    values: Vec::new(),
                    speaking_style: text("Measured."),
                    narrative_tags: Default::default(),
                },
                agent_profile: Some(agent_profile),
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
                    allowed_definitions: Default::default(),
                },
            }),
        ],
    };
    let registry = DefinitionRegistry::build(
        ContentPackContext {
            mod_id,
            mod_version: Version::new(1, 0, 0),
            pack_id,
            content_version: 1,
            content_hash: parse_content_hash("a".repeat(64)).expect("hash"),
        },
        [document],
    )
    .expect("registry");

    let player = ActorId::from(object_id("2b3c"));
    let scene = object_id("2b3d");
    let quay = object_id("2b3e");
    let inn = object_id("2b3f");
    let root = object_id("2b40");
    let coin = object_id("2b41");
    let grant = object_id("2b42");
    let origin = |id: &ContentDefinitionId| -> ContentOrigin {
        registry.get(id).expect("definition origin").origin.clone()
    };
    let mut resources = BTreeMap::new();
    resources.insert(
        resource_id.clone(),
        ResourcePool {
            resource_id: resource_id.clone(),
            current: Fixed::from_integer(10).expect("fixed"),
            base_maximum: Fixed::from_integer(10).expect("fixed"),
        },
    );
    let records = vec![
        DomainRecord::WorldState(WorldStateRecord {
            id: "wld_01890f6a-2b43-7d4e-8f90-123456789abc"
                .parse::<WorldId>()
                .expect("world id"),
            player_actor: player,
            active_scene: scene,
            clock: WorldTime::ZERO,
            rng_seed: [3; 32],
        }),
        DomainRecord::Scene(SceneRecord {
            id: scene,
            display_name: name("Harbor"),
            framing: text("Rain falls over the harbor."),
            entry_place: quay,
            active: true,
            origin: origin(&scene_definition),
        }),
        DomainRecord::Place(PlaceRecord {
            id: quay,
            scene_id: scene,
            display_name: name("Quay"),
            description: text("Wet stone."),
            tags: Default::default(),
            origin: origin(&quay_definition),
        }),
        DomainRecord::Place(PlaceRecord {
            id: inn,
            scene_id: scene,
            display_name: name("Inn"),
            description: text("A warm common room."),
            tags: Default::default(),
            origin: origin(&inn_definition),
        }),
        DomainRecord::Character(CharacterRecord {
            id: player,
            display_name: name("Traveler"),
            profile: CharacterProfile {
                summary: text("A traveler."),
                values: Vec::new(),
                speaking_style: text("Direct."),
                narrative_tags: Default::default(),
            },
            controller: CharacterController::Player,
            lifetime: CharacterLifetime::Persistent,
            location: quay,
            inventory_root: root,
            agent_binding: None,
            base_attributes: BaseAttributes::default(),
            attribute_adjustments: Vec::new(),
            resources,
            life_state: LifeState::Alive,
            action_state: ActionState::Idle,
            posture: Posture::Standing,
            origin: EntityOrigin::System {
                source: definition_id("system", "bootstrap"),
            },
        }),
        DomainRecord::Item(ItemRecord {
            id: root,
            definition_id: root_definition.clone(),
            stack: StackState(NonZeroU32::MIN),
            durability: None,
            container: Some(loreloom_core::ContainerState {
                max_weight_grams: Fixed::from_integer(10_000).expect("fixed"),
                max_children: 32,
            }),
            contained_by: None,
            owned_by: Some(player),
            equipped: None,
            located_at: Some(quay),
            custom_name: None,
            bound_actor: Some(player),
            parameters: BTreeMap::new(),
            instance_adjustments: Vec::new(),
            origin: EntityOrigin::Content {
                origin: origin(&root_definition),
            },
        }),
        DomainRecord::Item(ItemRecord {
            id: coin,
            definition_id: coin_definition,
            stack: StackState(NonZeroU32::new(3).expect("non-zero")),
            durability: None,
            container: None,
            contained_by: Some(root),
            owned_by: Some(player),
            equipped: None,
            located_at: None,
            custom_name: None,
            bound_actor: None,
            parameters: BTreeMap::new(),
            instance_adjustments: Vec::new(),
            origin: EntityOrigin::Content {
                origin: origin(&definition_id("item", "coin")),
            },
        }),
        DomainRecord::SkillGrant(SkillGrantRecord {
            id: grant,
            owner_id: player,
            skill_id: skill_id.clone(),
            rank: 1,
            proficiency: 0,
            source: SkillSource::Rule {
                rule_id: definition_id("rule", "bootstrap"),
            },
            enabled: true,
            ready_at: None,
            origin: EntityOrigin::Content {
                origin: origin(&skill_id),
            },
        }),
    ];
    Fixture {
        registry,
        records,
        config: WorldConfig {
            inventory_root_definition: root_definition,
            spawn_system_definition: definition_id("system", "spawn"),
            rule_limits: Default::default(),
        },
        player,
        scene,
        forest_scene_definition,
        quay,
        inn,
        root,
        coin,
        grant,
    }
}

#[test]
fn scene_transition_materializes_once_and_reactivates_preserved_scene_state() {
    let fixture = fixture();
    let mut world = GameWorld::from_records(
        Revision::ZERO,
        fixture.records,
        fixture.config.clone(),
        &fixture.registry,
    )
    .expect("world");
    let mut ids = SystemIdGenerator;

    let first = world
        .execute(
            WorldCommand {
                action_id: action_id("2b80"),
                actor_id: fixture.player,
                expected_revision: Revision::ZERO,
                kind: WorldCommandKind::TransitionScene {
                    target: SceneTransitionTarget::Definition {
                        scene_definition_id: fixture.forest_scene_definition.clone(),
                    },
                },
            },
            &fixture.registry,
            &mut ids,
        )
        .expect("materialize and enter forest");
    let forest_scene = world.world_state().active_scene;
    assert_ne!(forest_scene, fixture.scene);
    assert_eq!(first.revision, Revision::new(1));
    assert!(first.events.iter().any(|event| {
        event.kind
            == WorldEventKind::SceneLeft {
                scene_id: fixture.scene,
            }
    }));
    assert!(first.events.iter().any(|event| {
        event.kind
            == WorldEventKind::SceneEntered {
                scene_id: forest_scene,
            }
    }));

    let first_records = world.project_records().expect("project first transition");
    let forest_entry = first_records
        .iter()
        .find_map(|record| match record {
            DomainRecord::Scene(scene) if scene.id == forest_scene => Some(scene.entry_place),
            _ => None,
        })
        .expect("forest scene");
    assert_eq!(
        world.character(fixture.player).expect("player").location,
        forest_entry
    );
    assert_eq!(
        first_records
            .iter()
            .filter(|record| matches!(
                record,
                DomainRecord::Character(character)
                    if character.controller == CharacterController::Player
            ))
            .count(),
        1,
        "a scene bootstrap player entry must not create a second player"
    );
    let forest_npc = first_records
        .iter()
        .find_map(|record| match record {
            DomainRecord::Character(character)
                if character.id != fixture.player
                    && character.lifetime
                        == CharacterLifetime::Scene {
                            scene_id: forest_scene,
                        } =>
            {
                Some(character.id)
            }
            _ => None,
        })
        .expect("forest scene NPC");

    world
        .execute(
            WorldCommand {
                action_id: action_id("2b81"),
                actor_id: fixture.player,
                expected_revision: Revision::new(1),
                kind: WorldCommandKind::TransitionScene {
                    target: SceneTransitionTarget::Existing {
                        scene_id: fixture.scene,
                    },
                },
            },
            &fixture.registry,
            &mut ids,
        )
        .expect("return to harbor");
    assert!(world.character(forest_npc).is_some());

    world
        .execute(
            WorldCommand {
                action_id: action_id("2b82"),
                actor_id: fixture.player,
                expected_revision: Revision::new(2),
                kind: WorldCommandKind::TransitionScene {
                    target: SceneTransitionTarget::Definition {
                        scene_definition_id: fixture.forest_scene_definition,
                    },
                },
            },
            &fixture.registry,
            &mut ids,
        )
        .expect("re-enter materialized forest");
    let final_records = world.project_records().expect("project final transition");
    assert_eq!(world.world_state().active_scene, forest_scene);
    assert!(world.character(forest_npc).is_some());
    assert_eq!(
        final_records
            .iter()
            .filter(|record| matches!(record, DomainRecord::Scene(_)))
            .count(),
        2,
        "a content scene has one instance per world"
    );
    GameWorld::from_records(
        world.revision(),
        final_records,
        fixture.config,
        &fixture.registry,
    )
    .expect("rebuild transitioned world");
}

#[test]
fn rejected_scene_transition_restores_the_candidate_world() {
    let fixture = fixture();
    let mut world = GameWorld::from_records(
        Revision::ZERO,
        fixture.records,
        fixture.config,
        &fixture.registry,
    )
    .expect("world");
    let before = world.project_records().expect("before records");
    let error = world
        .execute(
            WorldCommand {
                action_id: action_id("2b83"),
                actor_id: fixture.player,
                expected_revision: Revision::ZERO,
                kind: WorldCommandKind::TransitionScene {
                    target: SceneTransitionTarget::Existing {
                        scene_id: fixture.scene,
                    },
                },
            },
            &fixture.registry,
            &mut SystemIdGenerator,
        )
        .expect_err("active scene cannot be re-entered");
    assert!(matches!(error, WorldError::DomainRule { .. }));
    assert_eq!(world.revision(), Revision::ZERO);
    assert_eq!(world.project_records().expect("after records"), before);
}

#[test]
fn world_executes_versioned_commands_and_rebuilds_without_entity_ids() {
    let fixture = fixture();
    let mut world = GameWorld::from_records(
        Revision::ZERO,
        fixture.records,
        fixture.config.clone(),
        &fixture.registry,
    )
    .expect("load world");
    let mut ids = SystemIdGenerator;
    let moved = world
        .execute(
            WorldCommand {
                action_id: action_id("2b50"),
                actor_id: fixture.player,
                expected_revision: Revision::ZERO,
                kind: WorldCommandKind::Move {
                    destination_id: fixture.inn,
                },
            },
            &fixture.registry,
            &mut ids,
        )
        .expect("move character");
    assert_eq!(moved.revision, Revision::new(1));
    assert_eq!(moved.record_ops().expect("record ops").len(), 1);
    assert_eq!(
        world.character(fixture.player).expect("player").location,
        fixture.inn
    );
    assert!(matches!(
        world.execute(
            WorldCommand {
                action_id: action_id("2b51"),
                actor_id: fixture.player,
                expected_revision: Revision::ZERO,
                kind: WorldCommandKind::Move {
                    destination_id: fixture.quay,
                },
            },
            &fixture.registry,
            &mut ids,
        ),
        Err(WorldError::Conflict { .. })
    ));

    let split = world
        .execute(
            WorldCommand {
                action_id: action_id("2b52"),
                actor_id: fixture.player,
                expected_revision: Revision::new(1),
                kind: WorldCommandKind::SplitStack {
                    item_id: fixture.coin,
                    quantity: 1,
                },
            },
            &fixture.registry,
            &mut ids,
        )
        .expect("split stack");
    assert_eq!(split.upserts.len(), 2);
    assert_eq!(world.item(fixture.coin).expect("coin").stack.0.get(), 2);

    let skill = world
        .execute(
            WorldCommand {
                action_id: action_id("2b53"),
                actor_id: fixture.player,
                expected_revision: Revision::new(2),
                kind: WorldCommandKind::UseSkill {
                    grant_id: fixture.grant,
                    target: SkillTargetRef::SelfTarget,
                },
            },
            &fixture.registry,
            &mut ids,
        )
        .expect("use skill");
    assert_eq!(skill.revision, Revision::new(3));
    let resource_id = definition_id("resource", "stamina");
    assert_eq!(
        world.character(fixture.player).expect("player").resources[&resource_id].current,
        Fixed::from_integer(9).expect("fixed")
    );
    let grant = world
        .project_records()
        .expect("project after skill")
        .into_iter()
        .find_map(|record| match record {
            DomainRecord::SkillGrant(grant) if grant.id == fixture.grant => Some(grant),
            _ => None,
        })
        .expect("skill grant projection");
    assert_eq!(grant.ready_at, Some(WorldTime::from_ticks(3)));
    assert!(matches!(
        world.execute(
            WorldCommand {
                action_id: action_id("2b54"),
                actor_id: fixture.player,
                expected_revision: Revision::new(3),
                kind: WorldCommandKind::UseSkill {
                    grant_id: fixture.grant,
                    target: SkillTargetRef::SelfTarget,
                },
            },
            &fixture.registry,
            &mut ids,
        ),
        Err(WorldError::DomainRule {
            rule: "skill_on_cooldown"
        })
    ));
    assert_eq!(world.revision(), Revision::new(3));
    let projected = world.project_records().expect("project records");
    let rebuilt = GameWorld::from_records(
        world.revision(),
        projected.clone(),
        fixture.config,
        &fixture.registry,
    )
    .expect("rebuild world");
    assert_eq!(rebuilt.project_records().expect("reproject"), projected);
}

#[test]
fn content_scene_bootstrap_uses_the_shared_factory_and_rebuilds_at_revision_zero() {
    let fixture = fixture();
    let plan = fixture
        .registry
        .compile_scene(&definition_id("scene", "harbor"))
        .expect("compile scene plan");
    let mut ids = SystemIdGenerator;
    let bootstrap = GameWorld::bootstrap(
        &plan,
        [7; 32],
        &fixture.registry,
        fixture.config.clone(),
        &mut ids,
    )
    .expect("bootstrap world");

    assert_eq!(bootstrap.characters.len(), 2);
    assert_eq!(
        bootstrap.player_actor,
        bootstrap.characters[&text("player")]
    );
    assert_eq!(
        bootstrap
            .records
            .iter()
            .filter(|record| matches!(record, DomainRecord::Character(_)))
            .count(),
        2
    );
    assert_eq!(
        bootstrap
            .records
            .iter()
            .filter(|record| matches!(record, DomainRecord::Item(_)))
            .count(),
        3
    );
    assert_eq!(
        bootstrap
            .records
            .iter()
            .filter(|record| matches!(record, DomainRecord::SkillGrant(_)))
            .count(),
        1
    );
    let rebuilt = GameWorld::from_records(
        Revision::ZERO,
        bootstrap.records.clone(),
        fixture.config,
        &fixture.registry,
    )
    .expect("rebuild bootstrap records");
    assert_eq!(rebuilt.revision(), Revision::ZERO);
    assert_eq!(
        rebuilt.project_records().expect("project bootstrap"),
        bootstrap.records
    );
}

#[test]
fn containment_rejects_non_container_without_mutation() {
    let fixture = fixture();
    let mut world = GameWorld::from_records(
        Revision::ZERO,
        fixture.records,
        fixture.config,
        &fixture.registry,
    )
    .expect("load world");
    let mut ids = SystemIdGenerator;
    assert!(matches!(
        world.execute(
            WorldCommand {
                action_id: action_id("2b60"),
                actor_id: fixture.player,
                expected_revision: Revision::ZERO,
                kind: WorldCommandKind::TransferItem {
                    item_id: fixture.root,
                    container_id: fixture.coin,
                },
            },
            &fixture.registry,
            &mut ids,
        ),
        Err(WorldError::WrongObjectKind { .. })
    ));
    assert_eq!(world.revision(), Revision::ZERO);
    assert_eq!(
        world.item(fixture.root).expect("root").located_at,
        Some(fixture.quay)
    );
    assert_eq!(fixture.scene, world.world_state().active_scene);
}

#[test]
fn scene_character_spawns_and_promotes_through_stable_records() {
    let fixture = fixture();
    let mut world = GameWorld::from_records(
        Revision::ZERO,
        fixture.records,
        fixture.config,
        &fixture.registry,
    )
    .expect("load world");
    let spec = CharacterSpawnSpec {
        origin: EntityOrigin::System {
            source: definition_id("system", "narrator"),
        },
        display_name: name("Dockhand"),
        profile: CharacterProfile {
            summary: text("A rain-soaked dockhand."),
            values: Vec::new(),
            speaking_style: text("Weathered."),
            narrative_tags: Default::default(),
        },
        controller: CharacterController::NarratorProxy,
        lifetime: CharacterLifetime::Scene {
            scene_id: fixture.scene,
        },
        agent_binding: None,
        placement: PlacementInput {
            scene_id: fixture.scene,
            place_id: fixture.quay,
        },
        attributes: BaseAttributes::default(),
        resources: BTreeMap::new(),
        conditions: Vec::new(),
        inventory: Vec::new(),
        skills: Vec::new(),
        knowledge: Vec::new(),
        goals: Vec::new(),
        trusted_constraints: SpawnConstraints {
            minimum_attributes: BTreeMap::new(),
            maximum_attributes: BTreeMap::new(),
            maximum_attribute_points: Fixed::ZERO,
            maximum_items: 0,
            maximum_skills: 0,
            allowed_definitions: Default::default(),
        },
    };
    let mut ids = SystemIdGenerator;
    let spawned = world
        .execute(
            WorldCommand {
                action_id: action_id("2b70"),
                actor_id: fixture.player,
                expected_revision: Revision::ZERO,
                kind: WorldCommandKind::SpawnCharacter {
                    spec: Box::new(spec),
                },
            },
            &fixture.registry,
            &mut ids,
        )
        .expect("spawn scene character");
    let spawned_actor = spawned
        .events
        .iter()
        .find_map(|event| match event.kind {
            WorldEventKind::CharacterSpawned { character_id } => Some(character_id),
            _ => None,
        })
        .expect("spawn event identifies character");
    assert!(matches!(
        world.character(spawned_actor).expect("spawned character").lifetime,
        CharacterLifetime::Scene { scene_id } if scene_id == fixture.scene
    ));

    world
        .execute(
            WorldCommand {
                action_id: action_id("2b71"),
                actor_id: fixture.player,
                expected_revision: Revision::new(1),
                kind: WorldCommandKind::PromoteCharacter {
                    actor_id: spawned_actor,
                },
            },
            &fixture.registry,
            &mut ids,
        )
        .expect("promote scene character");
    assert_eq!(
        world
            .character(spawned_actor)
            .expect("promoted character")
            .lifetime,
        CharacterLifetime::Persistent
    );
    assert_eq!(world.revision(), Revision::new(2));
}

#[test]
fn trusted_runtime_appends_versioned_transcript_records() {
    let fixture = fixture();
    let mut world = GameWorld::from_records(
        Revision::ZERO,
        fixture.records,
        fixture.config,
        &fixture.registry,
    )
    .expect("load world");
    let item = TranscriptItemRecord {
        id: "trn_01890f6a-2b80-7d4e-8f90-123456789abc"
            .parse::<TranscriptItemId>()
            .expect("transcript id"),
        session_id: "ses_01890f6a-2b81-7d4e-8f90-123456789abc"
            .parse()
            .expect("session id"),
        revision: Some(Revision::new(1)),
        speaker: TranscriptSpeaker::Player {
            actor_id: fixture.player,
            display_name: name("Traveler"),
        },
        text: loreloom_core::LongText::new("I listen at the inn door.").expect("text"),
        state: TranscriptState::Committed,
        supporting_events: Vec::new(),
    };
    let mut ids = SystemIdGenerator;
    let changes = world
        .execute(
            WorldCommand {
                action_id: action_id("2b82"),
                actor_id: fixture.player,
                expected_revision: Revision::ZERO,
                kind: WorldCommandKind::AppendTranscript {
                    items: vec![item.clone()],
                },
            },
            &fixture.registry,
            &mut ids,
        )
        .expect("append transcript");
    assert_eq!(changes.revision, Revision::new(1));
    assert!(changes.events.is_empty());
    assert_eq!(world.transcripts().collect::<Vec<_>>(), vec![&item]);
}
