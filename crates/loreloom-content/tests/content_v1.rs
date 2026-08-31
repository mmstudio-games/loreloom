use std::{collections::BTreeSet, num::NonZeroU32};

use loreloom_content::{
    AgentProfileDefinition, AttributeDefinition, CONTENT_SCHEMA_V1, CharacterCompileRequest,
    CharacterDefinition, ContentDocument, ContentError, ContentPackContext, Definition,
    DefinitionRegistry, DraftCompileRequest, EffectDefinition, GameplayActionDefinition,
    GenerationPolicy, InitialCharacterController, InitialCharacterLifetime, InitialFact,
    InitialItem, InitialResource, ItemDefinition, NpcDraft, ParameterDefinition,
    ParameterPersistence, ParameterType, ParameterVisibility, PlaceDefinition, PredicateDefinition,
    ResourceCost, ResourceDefinition, ResourceMaximumPolicy, RuleDefinition,
    SceneCharacterDefinition, SceneDefinition, SkillDefinition, SkillKind, SkillTarget,
    TagDefinition, TriggerDefinition, parse_content_hash,
};
use loreloom_core::{
    AttributeOperation, AutonomyMode, BaseAttributes, CharacterController, CharacterLifetime,
    CharacterProfile, ContentDefinitionId, DisplayName, EventId, FactSubject, FactValue, Fixed,
    GeneratedOrigin, GenerationId, ModId, ObjectId, ParameterValue, ShortText, SpawnConstraints,
};
use semver::Version;

fn id(kind: &str, key: &str) -> ContentDefinitionId {
    format!("games.loreloom.demo:{kind}/{key}")
        .parse()
        .expect("definition id")
}

fn text(value: &str) -> ShortText {
    ShortText::new(value).expect("short text")
}

fn name(value: &str) -> DisplayName {
    DisplayName::new(value).expect("display name")
}

fn context() -> ContentPackContext {
    ContentPackContext {
        mod_id: ModId::parse("games.loreloom.demo").expect("mod id"),
        mod_version: Version::new(1, 0, 0),
        pack_id: id("pack", "demo"),
        content_version: 1,
        content_hash: parse_content_hash("a".repeat(64)).expect("content hash"),
    }
}

fn fixture_document() -> ContentDocument {
    let attribute_id = id("attribute", "resolve");
    let resource_id = id("resource", "stamina");
    let item_id = id("item", "coin");
    let skill_id = id("skill", "focus");
    let agent_id = id("agent_profile", "resident");
    let fact_predicate = id("tag", "witnessed_rain");
    let character_id = id("character", "mara");
    let place_id = id("place", "quay");
    let scene_id = id("scene", "harbor");

    ContentDocument {
        schema_version: CONTENT_SCHEMA_V1,
        definitions: vec![
            Definition::Scene(SceneDefinition {
                id: scene_id,
                display_name: name("Harbor"),
                framing: text("A rain-dark harbor."),
                entry_place: place_id.clone(),
                places: BTreeSet::from([place_id.clone()]),
                characters: vec![SceneCharacterDefinition {
                    local_key: text("mara"),
                    character_id: character_id.clone(),
                    place_id: place_id.clone(),
                    controller: InitialCharacterController::Player,
                    lifetime: InitialCharacterLifetime::Persistent,
                }],
            }),
            Definition::Character(CharacterDefinition {
                id: character_id,
                display_name: name("Mara"),
                profile: CharacterProfile {
                    summary: text("A patient harbor warden."),
                    values: vec![text("Keep travelers safe.")],
                    speaking_style: text("Measured and direct."),
                    narrative_tags: BTreeSet::new(),
                },
                agent_profile: Some(agent_id.clone()),
                base_attributes: BaseAttributes::default(),
                resources: vec![InitialResource {
                    resource_id: resource_id.clone(),
                    current: Fixed::from_integer(8).expect("fixed"),
                    base_maximum: Fixed::from_integer(10).expect("fixed"),
                }],
                conditions: Vec::new(),
                inventory: vec![InitialItem {
                    local_key: text("coin"),
                    item_id: item_id.clone(),
                    quantity: NonZeroU32::new(3).expect("non-zero"),
                    parent_local_key: None,
                }],
                skills: vec![loreloom_content::InitialSkill {
                    skill_id: skill_id.clone(),
                    rank: NonZeroU32::new(1).expect("non-zero"),
                    proficiency: 0,
                    enabled: true,
                }],
                knowledge: vec![InitialFact {
                    subject: FactSubject::World,
                    predicate_id: fact_predicate.clone(),
                    value: FactValue::Bool(true),
                    confidence: Fixed::ONE,
                }],
                goals: Vec::new(),
                spawn_constraints: SpawnConstraints {
                    minimum_attributes: Default::default(),
                    maximum_attributes: Default::default(),
                    maximum_attribute_points: Fixed::from_integer(20).expect("fixed"),
                    maximum_items: 8,
                    maximum_skills: 4,
                    allowed_definitions: BTreeSet::from([item_id.clone(), skill_id.clone()]),
                },
            }),
            Definition::Skill(SkillDefinition {
                id: skill_id,
                display_name: name("Focus"),
                description: text("Regain composure."),
                kind: SkillKind::Active,
                costs: vec![ResourceCost {
                    resource_id: resource_id.clone(),
                    amount: Fixed::from_integer(1).expect("fixed"),
                }],
                cooldown_ticks: 3,
                target: SkillTarget::SelfTarget,
                executor_id: id("skill_executor", "effects"),
                effects: Vec::new(),
                reaction: None,
            }),
            Definition::Item(ItemDefinition {
                id: item_id,
                display_name: name("Coin"),
                description: text("A small brass coin."),
                tags: BTreeSet::new(),
                stack_limit: NonZeroU32::new(100).expect("non-zero"),
                unit_weight_grams: Fixed::from_integer(5).expect("fixed"),
                durability: None,
                container: None,
                equipment_slots: BTreeSet::new(),
                modifiers: Vec::new(),
            }),
            Definition::Resource(ResourceDefinition {
                id: resource_id,
                display_name: name("Stamina"),
                minimum: Fixed::ZERO,
                maximum: Fixed::from_integer(100).expect("fixed"),
                maximum_policy: ResourceMaximumPolicy::ClampCurrent,
                derived_from_attribute: None,
            }),
            Definition::Attribute(AttributeDefinition {
                id: attribute_id,
                display_name: name("Resolve"),
                minimum: Fixed::ZERO,
                maximum: Fixed::from_integer(20).expect("fixed"),
                allowed_operations: BTreeSet::from([
                    AttributeOperation::Flat,
                    AttributeOperation::Multiply,
                ]),
            }),
            Definition::AgentProfile(AgentProfileDefinition {
                id: agent_id,
                display_name: name("Resident"),
                system_style: text("Stay in character."),
                model_alias: text("default"),
                tool_capabilities: BTreeSet::new(),
                autonomy: AutonomyMode::Directed,
            }),
            Definition::Tag(TagDefinition {
                id: fact_predicate,
                display_name: name("Witnessed rain"),
            }),
            Definition::Place(PlaceDefinition {
                id: place_id,
                display_name: name("Quay"),
                description: text("Wet stone beside dark water."),
                tags: BTreeSet::new(),
                edges: BTreeSet::new(),
            }),
        ],
    }
}

#[test]
fn content_v1_builds_deterministic_registry_and_compiles_spawn_spec() {
    let registry =
        DefinitionRegistry::build(context(), [fixture_document()]).expect("build registry");
    let scene_id: ObjectId = "obj_01890f6a-2b3c-7d4e-8f90-123456789abc"
        .parse()
        .expect("scene id");
    let place_id: ObjectId = "obj_01890f6a-2b3d-7d4e-8f90-123456789abc"
        .parse()
        .expect("place id");
    let spec = registry
        .compile_character(
            &id("character", "mara"),
            CharacterCompileRequest {
                scene_id,
                place_id,
                controller: CharacterController::Agent,
                lifetime: CharacterLifetime::Scene { scene_id },
            },
        )
        .expect("compile character");
    assert_eq!(spec.display_name.as_str(), "Mara");
    assert!(spec.agent_binding.is_some());
    assert_eq!(spec.inventory.len(), 1);
    assert_eq!(spec.skills.len(), 1);
    assert_eq!(spec.knowledge.len(), 1);
    assert_eq!(spec.placement.place_id, place_id);

    let ids = registry
        .iter()
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn content_v1_compiles_an_owned_deterministic_scene_plan() {
    let registry =
        DefinitionRegistry::build(context(), [fixture_document()]).expect("build registry");
    let plan = registry
        .compile_scene(&id("scene", "harbor"))
        .expect("compile scene");

    assert_eq!(plan.definition_id, id("scene", "harbor"));
    assert_eq!(plan.entry_place, id("place", "quay"));
    assert_eq!(plan.places.len(), 1);
    assert_eq!(plan.places[0].definition_id, id("place", "quay"));
    assert_eq!(plan.characters.len(), 1);
    assert_eq!(plan.characters[0].local_key, text("mara"));
    assert_eq!(plan.characters[0].controller, CharacterController::Player);
    assert_eq!(
        plan.characters[0].lifetime,
        InitialCharacterLifetime::Persistent
    );
}

#[test]
fn content_v1_rejects_duplicate_scene_keys_and_agent_entries_without_profiles() {
    let mut duplicate = fixture_document();
    let Definition::Scene(scene) = &mut duplicate.definitions[0] else {
        panic!("fixture scene position");
    };
    scene.characters.push(scene.characters[0].clone());
    assert!(matches!(
        DefinitionRegistry::build(context(), [duplicate]),
        Err(ContentError::InvalidValue {
            field: "scene.character.local_key",
            ..
        })
    ));

    let mut unbound_agent = fixture_document();
    let Definition::Scene(scene) = &mut unbound_agent.definitions[0] else {
        panic!("fixture scene position");
    };
    scene.characters[0].controller = InitialCharacterController::Agent;
    let Definition::Character(character) = &mut unbound_agent.definitions[1] else {
        panic!("fixture character position");
    };
    character.agent_profile = None;
    assert!(matches!(
        DefinitionRegistry::build(context(), [unbound_agent]),
        Err(ContentError::InvalidValue {
            field: "scene.character.agent_profile",
            ..
        })
    ));
}

#[test]
fn content_v1_rejects_unknown_fields_and_wrong_definition_kind() {
    let mut value = serde_json::to_value(fixture_document()).expect("serialize document");
    value
        .as_object_mut()
        .expect("document object")
        .insert("future_control".into(), serde_json::json!(true));
    assert!(serde_json::from_value::<ContentDocument>(value).is_err());

    let mut document = fixture_document();
    let Definition::Attribute(attribute) = &mut document.definitions[5] else {
        panic!("fixture attribute position");
    };
    attribute.id = id("item", "resolve");
    assert!(matches!(
        DefinitionRegistry::build(context(), [document]),
        Err(ContentError::DefinitionKind { .. })
    ));
}

#[test]
fn content_v1_rejects_missing_cross_reference_before_registry_publish() {
    let mut document = fixture_document();
    let Definition::Character(character) = &mut document.definitions[1] else {
        panic!("fixture character position");
    };
    let missing = id("item", "missing");
    character.inventory[0].item_id = missing.clone();
    character
        .spawn_constraints
        .allowed_definitions
        .insert(missing);
    assert!(matches!(
        DefinitionRegistry::build(context(), [document]),
        Err(ContentError::InvalidReference { .. })
    ));
}

#[test]
fn runtime_draft_uses_trusted_generation_policy_and_same_spawn_spec() {
    let registry =
        DefinitionRegistry::build(context(), [fixture_document()]).expect("build registry");
    let character_entry = registry
        .get(&id("character", "mara"))
        .expect("character definition");
    let Definition::Character(character) = &character_entry.definition else {
        panic!("character definition kind");
    };
    let draft = NpcDraft {
        display_name: name("Generated Mara"),
        profile: character.profile.clone(),
        agent_profile: character.agent_profile.clone(),
        base_attributes: character.base_attributes.clone(),
        resources: character.resources.clone(),
        conditions: character.conditions.clone(),
        inventory: character.inventory.clone(),
        skills: character.skills.clone(),
        knowledge: character.knowledge.clone(),
        goals: character.goals.clone(),
    };
    let policy = GenerationPolicy {
        id: id("generation_policy", "resident"),
        constraints: character.spawn_constraints.clone(),
        allowed_agent_profiles: BTreeSet::from([id("agent_profile", "resident")]),
    };
    let scene_id: ObjectId = "obj_01890f6a-2b3c-7d4e-8f90-123456789abc"
        .parse()
        .expect("scene id");
    let place_id: ObjectId = "obj_01890f6a-2b3d-7d4e-8f90-123456789abc"
        .parse()
        .expect("place id");
    let request = DraftCompileRequest {
        origin: GeneratedOrigin {
            generation_id: "gen_01890f6a-2b3e-7d4e-8f90-123456789abc"
                .parse::<GenerationId>()
                .expect("generation id"),
            generator_version: text("draft-v1"),
            source_event: "evt_01890f6a-2b3f-7d4e-8f90-123456789abc"
                .parse::<EventId>()
                .expect("event id"),
        },
        scene_id,
        place_id,
        controller: CharacterController::Agent,
        lifetime: CharacterLifetime::Scene { scene_id },
    };
    let spec = registry
        .compile_draft(&draft, &policy, request.clone())
        .expect("compile generated draft");
    assert_eq!(spec.display_name.as_str(), "Generated Mara");
    assert!(matches!(
        spec.origin,
        loreloom_core::EntityOrigin::Generated { .. }
    ));

    let mut restricted = policy;
    restricted.constraints.allowed_definitions.clear();
    assert!(matches!(
        registry.compile_draft(&draft, &restricted, request),
        Err(ContentError::InvalidValue { .. })
    ));
}

#[test]
fn declarative_rules_reference_registered_generic_parameters_and_actions() {
    let parameter_id = id("parameter", "rain_count");
    let action_id = id("gameplay_action", "wait_in_rain");
    let mut document = fixture_document();
    document
        .definitions
        .push(Definition::Parameter(ParameterDefinition {
            id: parameter_id.clone(),
            display_name: name("Rain count"),
            value_type: ParameterType::Counter {
                minimum: 0,
                maximum: 100,
            },
            default: ParameterValue::Counter(0),
            visibility: ParameterVisibility::Public,
            persistence: ParameterPersistence::Save,
        }));
    document
        .definitions
        .push(Definition::GameplayAction(GameplayActionDefinition {
            id: action_id.clone(),
            display_name: name("Wait in rain"),
            capability: text("gameplay.action"),
            parameters: Vec::new(),
            predicates: Vec::new(),
            effects: vec![EffectDefinition::SetParameter {
                parameter_id: parameter_id.clone(),
                value: ParameterValue::Counter(1),
            }],
        }));
    document.definitions.push(Definition::Rule(RuleDefinition {
        id: id("rule", "rain_rule"),
        priority: 0,
        trigger: TriggerDefinition::GameplayAction {
            action_id: action_id.clone(),
        },
        predicates: Vec::new(),
        effects: vec![EffectDefinition::SetParameter {
            parameter_id,
            value: ParameterValue::Counter(2),
        }],
    }));
    let registry = DefinitionRegistry::build(context(), [document]).expect("build rule registry");
    assert!(registry.get(&action_id).is_some());
}

#[test]
fn declarative_registry_rejects_nested_predicate_budget_and_static_emit_cycles() {
    let mut predicate = PredicateDefinition::HasTag {
        tag_id: id("tag", "deep"),
    };
    for _ in 0..64 {
        predicate = PredicateDefinition::Not {
            predicate: Box::new(predicate),
        };
    }
    let mut over_budget = fixture_document();
    over_budget
        .definitions
        .push(Definition::GameplayAction(GameplayActionDefinition {
            id: id("gameplay_action", "too_deep"),
            display_name: name("Too deep"),
            capability: text("gameplay.deep"),
            parameters: Vec::new(),
            predicates: vec![predicate],
            effects: Vec::new(),
        }));
    assert!(matches!(
        DefinitionRegistry::build(context(), [over_budget]),
        Err(ContentError::InvalidValue {
            field: "gameplay_action.budget",
            ..
        })
    ));

    let mut cycle = fixture_document();
    cycle.definitions.push(Definition::Rule(RuleDefinition {
        id: id("rule", "cycle_a"),
        priority: 0,
        trigger: TriggerDefinition::WorldEvent {
            event_type: text("cycle_a"),
        },
        predicates: Vec::new(),
        effects: vec![EffectDefinition::EmitEvent {
            event_type: text("cycle_b"),
        }],
    }));
    cycle.definitions.push(Definition::Rule(RuleDefinition {
        id: id("rule", "cycle_b"),
        priority: 0,
        trigger: TriggerDefinition::WorldEvent {
            event_type: text("cycle_b"),
        },
        predicates: Vec::new(),
        effects: vec![EffectDefinition::EmitEvent {
            event_type: text("cycle_a"),
        }],
    }));
    assert!(matches!(
        DefinitionRegistry::build(context(), [cycle]),
        Err(ContentError::InvalidValue {
            field: "rule.emit_cycle",
            ..
        })
    ));
}
