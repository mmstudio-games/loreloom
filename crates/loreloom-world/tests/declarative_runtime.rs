use std::{
    collections::BTreeMap,
    num::{NonZeroU32, NonZeroU64},
};

use loreloom_content::{
    ActionParameterDefinition, CONTENT_SCHEMA_V1, CharacterDefinition, ConditionDefinition,
    ContainerDefinition, ContentDocument, ContentPackContext, Definition, DefinitionRegistry,
    DurationPolicy, EffectDefinition, EventDefinition, EventNodeDefinition, EventOptionDefinition,
    GameplayActionDefinition, InitialCharacterController, InitialCharacterLifetime, InitialSkill,
    ItemDefinition, ParameterDefinition, ParameterPersistence, ParameterType, ParameterVisibility,
    PredicateDefinition, ResourceCost, ResourceDefinition, ResourceMaximumPolicy, RuleDefinition,
    SceneCharacterDefinition, SceneDefinition, SkillDefinition, SkillKind, SkillTarget,
    StackPolicy, TriggerDefinition, parse_content_hash,
};
use loreloom_core::{
    ActionId, BaseAttributes, CharacterProfile, ContentDefinitionId, DisplayName, DomainRecord,
    EventInstanceRecord, EventStatus, Fixed, IntensityPolicy, ModId, ObjectId, ParameterValue,
    Revision, ShortText, SkillTargetRef, SpawnConstraints, SystemIdGenerator, WorldCommand,
    WorldCommandKind, WorldEventKind, WorldTime,
};
use loreloom_world::{GameWorld, RuleLimits, WorldConfig, WorldError};
use semver::Version;

fn id(kind: &str, key: &str) -> ContentDefinitionId {
    format!("games.loreloom.declarative:{kind}/{key}")
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
    config: WorldConfig,
    records: Vec<DomainRecord>,
    player: loreloom_core::ActorId,
    event_instance: ObjectId,
}

fn fixture(limits: RuleLimits) -> Fixture {
    let pack_id = id("pack", "core");
    let resource_id = id("resource", "stamina");
    let root_id = id("item", "inventory_root");
    let reward_id = id("item", "reward");
    let skill_id = id("skill", "insight");
    let active_skill_id = id("skill", "archive_ritual");
    let condition_id = id("condition", "focus");
    let save_parameter = id("parameter", "story_step");
    let session_parameter = id("parameter", "hint_seen");
    let action_parameter = id("action_parameter", "effort");
    let action_definition = id("gameplay_action", "study");
    let object_action = id("gameplay_action", "inspect_place");
    let object_parameter = id("action_parameter", "place");
    let grant_action = id("gameplay_action", "claim_reward");
    let event_definition = id("event", "question");
    let event_node = id("event_node", "question_start");
    let event_option = id("event_option", "answer");
    let place_definition = id("place", "library");
    let scene_definition = id("scene", "archive");
    let character_definition = id("character", "reader");
    let rule_audit = id("rule", "audit_after_study");
    let rule_clock = id("rule", "clock_pulse");
    let rule_a = id("rule", "emit_after_study");
    let rule_b = id("rule", "record_chain");
    let rule_skill = id("rule", "record_skill_use");

    let document = ContentDocument {
        schema_version: CONTENT_SCHEMA_V1,
        definitions: vec![
            Definition::Resource(ResourceDefinition {
                id: resource_id.clone(),
                display_name: name("Stamina"),
                minimum: Fixed::ZERO,
                maximum: Fixed::from_integer(10).expect("fixed"),
                maximum_policy: ResourceMaximumPolicy::ClampCurrent,
                derived_from_attribute: None,
            }),
            Definition::Item(ItemDefinition {
                id: root_id.clone(),
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
                id: reward_id.clone(),
                display_name: name("Archive token"),
                description: text("A stamped brass token."),
                tags: Default::default(),
                stack_limit: NonZeroU32::new(10).expect("non-zero"),
                unit_weight_grams: Fixed::ONE,
                durability: None,
                container: None,
                equipment_slots: Default::default(),
                modifiers: Vec::new(),
            }),
            Definition::Skill(SkillDefinition {
                id: skill_id.clone(),
                display_name: name("Insight"),
                description: text("Notice hidden connections."),
                kind: SkillKind::Passive,
                costs: Vec::new(),
                cooldown_ticks: 0,
                target: SkillTarget::SelfTarget,
                executor_id: id("skill_executor", "none"),
                effects: Vec::new(),
                reaction: None,
            }),
            Definition::Skill(SkillDefinition {
                id: active_skill_id.clone(),
                display_name: name("Archive ritual"),
                description: text("Trade stamina for an archive blessing."),
                kind: SkillKind::Active,
                costs: vec![ResourceCost {
                    resource_id: resource_id.clone(),
                    amount: Fixed::from_integer(2).expect("fixed"),
                }],
                cooldown_ticks: 4,
                target: SkillTarget::SelfTarget,
                executor_id: id("skill_executor", "declarative"),
                effects: vec![
                    EffectDefinition::ResourceDelta {
                        resource_id: resource_id.clone(),
                        amount: Fixed::ONE,
                    },
                    EffectDefinition::ApplyCondition {
                        condition_id: condition_id.clone(),
                        stacks: NonZeroU32::MIN,
                        intensity: Fixed::ONE,
                    },
                    EffectDefinition::GrantItem {
                        item_id: reward_id.clone(),
                        quantity: NonZeroU32::MIN,
                    },
                    EffectDefinition::GrantSkill {
                        skill_id: skill_id.clone(),
                        rank: NonZeroU32::MIN,
                    },
                    EffectDefinition::SetParameter {
                        parameter_id: save_parameter.clone(),
                        value: ParameterValue::Counter(9),
                    },
                    EffectDefinition::EmitEvent {
                        event_type: text("archive_ritual_completed"),
                    },
                ],
                reaction: None,
            }),
            Definition::Condition(ConditionDefinition {
                id: condition_id.clone(),
                display_name: name("Focused"),
                tags: Default::default(),
                stack_policy: StackPolicy::IncreaseStacks {
                    maximum: NonZeroU32::new(3).expect("non-zero"),
                    refresh_duration: true,
                },
                intensity_policy: IntensityPolicy::Maximum,
                duration: DurationPolicy::Finite {
                    ticks: NonZeroU64::new(5).expect("non-zero"),
                },
                symptoms: Vec::new(),
                modifiers: Vec::new(),
                periodic: None,
            }),
            Definition::Parameter(ParameterDefinition {
                id: save_parameter.clone(),
                display_name: name("Story step"),
                value_type: ParameterType::Counter {
                    minimum: 0,
                    maximum: 10,
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
                visibility: ParameterVisibility::Narrator,
                persistence: ParameterPersistence::Session,
            }),
            Definition::GameplayAction(GameplayActionDefinition {
                id: action_definition.clone(),
                display_name: name("Study"),
                capability: text("gameplay.study"),
                parameters: vec![ActionParameterDefinition {
                    id: action_parameter.clone(),
                    value_type: ParameterType::Counter {
                        minimum: 1,
                        maximum: 3,
                    },
                    required: true,
                }],
                predicates: vec![PredicateDefinition::ResourceAtLeast {
                    resource_id: resource_id.clone(),
                    amount: Fixed::ONE,
                }],
                effects: vec![
                    EffectDefinition::ResourceDelta {
                        resource_id: resource_id.clone(),
                        amount: Fixed::from_integer(-1).expect("fixed"),
                    },
                    EffectDefinition::ApplyCondition {
                        condition_id: condition_id.clone(),
                        stacks: NonZeroU32::MIN,
                        intensity: Fixed::ONE,
                    },
                    EffectDefinition::SetParameter {
                        parameter_id: save_parameter.clone(),
                        value: ParameterValue::Counter(1),
                    },
                    EffectDefinition::SetParameter {
                        parameter_id: session_parameter.clone(),
                        value: ParameterValue::Bool(true),
                    },
                ],
            }),
            Definition::GameplayAction(GameplayActionDefinition {
                id: grant_action,
                display_name: name("Claim reward"),
                capability: text("gameplay.reward"),
                parameters: Vec::new(),
                predicates: Vec::new(),
                effects: vec![
                    EffectDefinition::GrantItem {
                        item_id: reward_id,
                        quantity: NonZeroU32::new(2).expect("non-zero"),
                    },
                    EffectDefinition::GrantSkill {
                        skill_id,
                        rank: NonZeroU32::new(2).expect("non-zero"),
                    },
                ],
            }),
            Definition::GameplayAction(GameplayActionDefinition {
                id: object_action,
                display_name: name("Inspect place"),
                capability: text("gameplay.inspect"),
                parameters: vec![ActionParameterDefinition {
                    id: object_parameter,
                    value_type: ParameterType::ObjectRef {
                        allowed_kinds: [text("place")].into_iter().collect(),
                    },
                    required: true,
                }],
                predicates: Vec::new(),
                effects: Vec::new(),
            }),
            Definition::Event(EventDefinition {
                id: event_definition.clone(),
                display_name: name("A question"),
                entry_node: event_node.clone(),
                nodes: vec![EventNodeDefinition {
                    id: event_node.clone(),
                    text: text("What is written here?"),
                    options: vec![
                        EventOptionDefinition {
                            id: event_option,
                            display_name: name("Answer"),
                            visible_if: vec![PredicateDefinition::ResourceAtLeast {
                                resource_id: resource_id.clone(),
                                amount: Fixed::from_integer(10).expect("fixed"),
                            }],
                            enabled_if: Vec::new(),
                            effects: vec![EffectDefinition::SetParameter {
                                parameter_id: save_parameter.clone(),
                                value: ParameterValue::Counter(7),
                            }],
                            next_node: None,
                        },
                        EventOptionDefinition {
                            id: id("event_option", "wait"),
                            display_name: name("Wait"),
                            visible_if: Vec::new(),
                            enabled_if: vec![PredicateDefinition::ResourceAtLeast {
                                resource_id: resource_id.clone(),
                                amount: Fixed::from_integer(10).expect("fixed"),
                            }],
                            effects: Vec::new(),
                            next_node: None,
                        },
                    ],
                }],
            }),
            Definition::Rule(RuleDefinition {
                id: rule_audit,
                priority: 0,
                trigger: TriggerDefinition::GameplayAction {
                    action_id: action_definition.clone(),
                },
                predicates: Vec::new(),
                effects: Vec::new(),
            }),
            Definition::Rule(RuleDefinition {
                id: rule_clock,
                priority: 0,
                trigger: TriggerDefinition::WorldClock {
                    every_ticks: NonZeroU64::new(2).expect("non-zero"),
                },
                predicates: Vec::new(),
                effects: vec![EffectDefinition::SetParameter {
                    parameter_id: save_parameter.clone(),
                    value: ParameterValue::Counter(5),
                }],
            }),
            Definition::Rule(RuleDefinition {
                id: rule_a,
                priority: 0,
                trigger: TriggerDefinition::GameplayAction {
                    action_id: action_definition.clone(),
                },
                predicates: Vec::new(),
                effects: vec![EffectDefinition::EmitEvent {
                    event_type: text("study_chain"),
                }],
            }),
            Definition::Rule(RuleDefinition {
                id: rule_b,
                priority: 0,
                trigger: TriggerDefinition::WorldEvent {
                    event_type: text("study_chain"),
                },
                predicates: Vec::new(),
                effects: vec![EffectDefinition::SetParameter {
                    parameter_id: save_parameter,
                    value: ParameterValue::Counter(2),
                }],
            }),
            Definition::Rule(RuleDefinition {
                id: rule_skill,
                priority: 0,
                trigger: TriggerDefinition::WorldEvent {
                    event_type: text("skill_used"),
                },
                predicates: Vec::new(),
                effects: vec![EffectDefinition::SetParameter {
                    parameter_id: id("parameter", "story_step"),
                    value: ParameterValue::Counter(10),
                }],
            }),
            Definition::Place(loreloom_content::PlaceDefinition {
                id: place_definition.clone(),
                display_name: name("Library"),
                description: text("Shelves vanish into shadow."),
                tags: Default::default(),
                edges: Default::default(),
            }),
            Definition::Scene(SceneDefinition {
                id: scene_definition.clone(),
                display_name: name("Archive"),
                framing: text("A silent archive."),
                entry_place: place_definition.clone(),
                places: [place_definition.clone()].into_iter().collect(),
                characters: vec![SceneCharacterDefinition {
                    local_key: text("player"),
                    character_id: character_definition.clone(),
                    place_id: place_definition,
                    controller: InitialCharacterController::Player,
                    lifetime: InitialCharacterLifetime::Persistent,
                }],
            }),
            Definition::Character(CharacterDefinition {
                id: character_definition,
                display_name: name("Reader"),
                profile: CharacterProfile {
                    summary: text("A patient reader."),
                    values: Vec::new(),
                    speaking_style: text("Quiet."),
                    narrative_tags: Default::default(),
                },
                agent_profile: None,
                base_attributes: BaseAttributes::default(),
                resources: vec![loreloom_content::InitialResource {
                    resource_id,
                    current: Fixed::from_integer(10).expect("fixed"),
                    base_maximum: Fixed::from_integer(10).expect("fixed"),
                }],
                conditions: Vec::new(),
                inventory: Vec::new(),
                skills: vec![InitialSkill {
                    skill_id: active_skill_id.clone(),
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
                    maximum_items: 0,
                    maximum_skills: 1,
                    allowed_definitions: [active_skill_id].into_iter().collect(),
                },
            }),
        ],
    };
    let registry = DefinitionRegistry::build(
        ContentPackContext {
            mod_id: ModId::parse("games.loreloom.declarative").expect("mod id"),
            mod_version: Version::new(1, 0, 0),
            pack_id,
            content_version: 1,
            content_hash: parse_content_hash("d".repeat(64)).expect("content hash"),
        },
        [document],
    )
    .expect("registry");
    let config = WorldConfig {
        inventory_root_definition: root_id,
        spawn_system_definition: id("system", "spawn"),
        rule_limits: limits,
    };
    let plan = registry
        .compile_scene(&scene_definition)
        .expect("scene plan");
    let mut ids = SystemIdGenerator;
    let bootstrap = GameWorld::bootstrap(&plan, [8; 32], &registry, config.clone(), &mut ids)
        .expect("bootstrap");
    let event_instance = object_id("6a10");
    let mut records = bootstrap.records;
    records.push(DomainRecord::EventInstance(EventInstanceRecord {
        id: event_instance,
        definition_id: event_definition,
        current_node: event_node,
        scene_id: Some(bootstrap.active_scene),
        started_at: WorldTime::ZERO,
        status: EventStatus::Active,
        committed_options: Vec::new(),
    }));
    Fixture {
        registry,
        config,
        records,
        player: bootstrap.player_actor,
        event_instance,
    }
}

fn command(
    fixture: &Fixture,
    expected_revision: Revision,
    suffix: &str,
    kind: WorldCommandKind,
) -> WorldCommand {
    WorldCommand {
        action_id: action_id(suffix),
        actor_id: fixture.player,
        expected_revision,
        kind,
    }
}

fn parameter(records: &[DomainRecord], parameter_id: &ContentDefinitionId) -> ParameterValue {
    records
        .iter()
        .find_map(|record| match record {
            DomainRecord::ParameterSet(set) => set.values.get(parameter_id).cloned(),
            _ => None,
        })
        .expect("saved parameter")
}

fn skill_grant(records: &[DomainRecord], skill_id: &ContentDefinitionId) -> ObjectId {
    records
        .iter()
        .find_map(|record| match record {
            DomainRecord::SkillGrant(grant) if &grant.skill_id == skill_id => Some(grant.id),
            _ => None,
        })
        .expect("skill grant")
}

#[test]
fn gameplay_action_applies_typed_effects_and_fifo_rules_in_one_revision() {
    let fixture = fixture(RuleLimits::default());
    let mut world = GameWorld::from_records(
        Revision::ZERO,
        fixture.records.clone(),
        fixture.config.clone(),
        &fixture.registry,
    )
    .expect("world");
    let mut ids = SystemIdGenerator;
    let changes = world
        .execute(
            command(
                &fixture,
                Revision::ZERO,
                "6a20",
                WorldCommandKind::PerformGameplayAction {
                    action_id: id("gameplay_action", "study"),
                    arguments: BTreeMap::from([(
                        id("action_parameter", "effort"),
                        ParameterValue::Counter(1),
                    )]),
                },
            ),
            &fixture.registry,
            &mut ids,
        )
        .expect("perform action");

    assert_eq!(changes.revision, Revision::new(1));
    assert_eq!(
        parameter(&changes.upserts, &id("parameter", "story_step")),
        ParameterValue::Counter(2)
    );
    assert_eq!(
        world.session_parameters()[&id("parameter", "hint_seen")],
        ParameterValue::Bool(true)
    );
    assert!(changes.upserts.iter().all(|record| match record {
        DomainRecord::ParameterSet(set) => {
            !set.values.contains_key(&id("parameter", "hint_seen"))
        }
        _ => true,
    }));
    let triggered = changes
        .events
        .iter()
        .filter_map(|event| match &event.kind {
            WorldEventKind::RuleTriggered { rule_id, .. } => Some(rule_id.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        triggered,
        vec![
            id("rule", "audit_after_study"),
            id("rule", "emit_after_study"),
            id("rule", "record_chain")
        ]
    );
    let records = world.project_records().expect("records");
    let condition = records
        .iter()
        .find_map(|record| match record {
            DomainRecord::Condition(value) => Some(value),
            _ => None,
        })
        .expect("condition");
    assert_eq!(condition.stacks.get(), 1);
    let mut rule_counts = records
        .iter()
        .filter_map(|record| match record {
            DomainRecord::RuleState(value) => Some(value.trigger_count),
            _ => None,
        })
        .collect::<Vec<_>>();
    rule_counts.sort_unstable();
    assert_eq!(rule_counts, vec![0, 0, 1, 1, 1]);

    let rebuilt = GameWorld::from_records(
        world.revision(),
        records,
        fixture.config.clone(),
        &fixture.registry,
    )
    .expect("rebuild");
    assert_eq!(
        rebuilt.session_parameters()[&id("parameter", "hint_seen")],
        ParameterValue::Bool(false)
    );
}

#[test]
fn active_skill_applies_cost_effect_plan_cooldown_and_rules_atomically() {
    let fixture = fixture(RuleLimits::default());
    let grant_id = skill_grant(&fixture.records, &id("skill", "archive_ritual"));
    let mut world = GameWorld::from_records(
        Revision::ZERO,
        fixture.records.clone(),
        fixture.config.clone(),
        &fixture.registry,
    )
    .expect("world");
    let changes = world
        .execute(
            command(
                &fixture,
                Revision::ZERO,
                "6a25",
                WorldCommandKind::UseSkill {
                    grant_id,
                    target: SkillTargetRef::SelfTarget,
                },
            ),
            &fixture.registry,
            &mut SystemIdGenerator,
        )
        .expect("use active skill");

    assert_eq!(changes.revision, Revision::new(1));
    assert_eq!(
        world.character(fixture.player).expect("player").resources[&id("resource", "stamina")]
            .current,
        Fixed::from_integer(9).expect("fixed")
    );
    assert_eq!(
        parameter(&changes.upserts, &id("parameter", "story_step")),
        ParameterValue::Counter(10)
    );
    for expected in [Fixed::from_integer(-2).expect("fixed"), Fixed::ONE] {
        assert!(changes.events.iter().any(|event| matches!(
            &event.kind,
            WorldEventKind::ResourceChanged { delta, .. } if *delta == expected
        )));
    }
    assert!(
        changes
            .events
            .iter()
            .any(|event| matches!(event.kind, WorldEventKind::ConditionApplied { .. }))
    );
    assert!(
        changes
            .events
            .iter()
            .any(|event| matches!(event.kind, WorldEventKind::ItemGranted { quantity: 1, .. }))
    );
    assert!(
        changes
            .events
            .iter()
            .any(|event| matches!(event.kind, WorldEventKind::SkillGranted { .. }))
    );
    assert!(changes.events.iter().any(|event| matches!(
        &event.kind,
        WorldEventKind::DeclarativeEventEmitted { event_type, .. }
            if event_type.as_str() == "archive_ritual_completed"
    )));
    assert!(
        changes
            .events
            .iter()
            .any(|event| matches!(event.kind, WorldEventKind::SkillUsed { .. }))
    );
    assert!(changes.events.iter().any(|event| matches!(
        &event.kind,
        WorldEventKind::RuleTriggered { rule_id, .. }
            if rule_id == &id("rule", "record_skill_use")
    )));

    let records = world.project_records().expect("records");
    let grant = records
        .iter()
        .find_map(|record| match record {
            DomainRecord::SkillGrant(grant) if grant.id == grant_id => Some(grant),
            _ => None,
        })
        .expect("active skill grant");
    assert_eq!(grant.ready_at, Some(WorldTime::from_ticks(4)));
    let rebuilt = GameWorld::from_records(
        changes.revision,
        records.clone(),
        fixture.config,
        &fixture.registry,
    )
    .expect("rebuild");
    assert_eq!(rebuilt.project_records().expect("rebuilt records"), records);
}

#[test]
fn active_skill_rolls_back_cost_and_partial_effects_when_budget_fails() {
    let fixture = fixture(RuleLimits {
        max_applied_effects: 3,
        ..RuleLimits::default()
    });
    let grant_id = skill_grant(&fixture.records, &id("skill", "archive_ritual"));
    let mut world = GameWorld::from_records(
        Revision::ZERO,
        fixture.records.clone(),
        fixture.config.clone(),
        &fixture.registry,
    )
    .expect("world");
    let before = world.project_records().expect("before");
    assert!(matches!(
        world.execute(
            command(
                &fixture,
                Revision::ZERO,
                "6a26",
                WorldCommandKind::UseSkill {
                    grant_id,
                    target: SkillTargetRef::SelfTarget,
                },
            ),
            &fixture.registry,
            &mut SystemIdGenerator,
        ),
        Err(WorldError::DomainRule {
            rule: "rule_effect_budget"
        })
    ));
    assert_eq!(world.revision(), Revision::ZERO);
    assert_eq!(world.project_records().expect("after"), before);
}

#[test]
fn gameplay_action_rejects_argument_schema_and_rolls_back_budget_failure() {
    let fixture = fixture(RuleLimits {
        max_triggered_rules: 1,
        ..RuleLimits::default()
    });
    let mut world = GameWorld::from_records(
        Revision::ZERO,
        fixture.records.clone(),
        fixture.config.clone(),
        &fixture.registry,
    )
    .expect("world");
    let before = world.project_records().expect("before");
    let mut ids = SystemIdGenerator;
    for (suffix, arguments) in [
        ("6a30", BTreeMap::new()),
        (
            "6a31",
            BTreeMap::from([(id("action_parameter", "extra"), ParameterValue::Counter(1))]),
        ),
        (
            "6a32",
            BTreeMap::from([(id("action_parameter", "effort"), ParameterValue::Bool(true))]),
        ),
    ] {
        assert!(matches!(
            world.execute(
                command(
                    &fixture,
                    Revision::ZERO,
                    suffix,
                    WorldCommandKind::PerformGameplayAction {
                        action_id: id("gameplay_action", "study"),
                        arguments,
                    },
                ),
                &fixture.registry,
                &mut ids,
            ),
            Err(WorldError::DomainRule { .. })
        ));
    }
    assert!(matches!(
        world.execute(
            command(
                &fixture,
                Revision::ZERO,
                "6a34",
                WorldCommandKind::PerformGameplayAction {
                    action_id: id("gameplay_action", "inspect_place"),
                    arguments: BTreeMap::from([(
                        id("action_parameter", "place"),
                        ParameterValue::ObjectRef(fixture.player.object_id()),
                    )]),
                },
            ),
            &fixture.registry,
            &mut ids,
        ),
        Err(WorldError::DomainRule {
            rule: "parameter_object_kind"
        })
    ));
    assert!(matches!(
        world.execute(
            command(
                &fixture,
                Revision::ZERO,
                "6a33",
                WorldCommandKind::PerformGameplayAction {
                    action_id: id("gameplay_action", "study"),
                    arguments: BTreeMap::from([(
                        id("action_parameter", "effort"),
                        ParameterValue::Counter(1),
                    )]),
                },
            ),
            &fixture.registry,
            &mut ids,
        ),
        Err(WorldError::DomainRule {
            rule: "rule_trigger_budget"
        })
    ));
    assert_eq!(world.revision(), Revision::ZERO);
    assert_eq!(world.project_records().expect("after"), before);
    assert_eq!(
        world.session_parameters()[&id("parameter", "hint_seen")],
        ParameterValue::Bool(false)
    );
}

#[test]
fn event_option_is_revalidated_and_commits_effect_history_and_completion_together() {
    let fixture = fixture(RuleLimits::default());
    let mut world = GameWorld::from_records(
        Revision::ZERO,
        fixture.records.clone(),
        fixture.config.clone(),
        &fixture.registry,
    )
    .expect("world");
    let mut ids = SystemIdGenerator;
    let chosen = world
        .execute(
            command(
                &fixture,
                Revision::ZERO,
                "6a40",
                WorldCommandKind::ChooseEventOption {
                    event_instance_id: fixture.event_instance,
                    option_id: id("event_option", "answer"),
                },
            ),
            &fixture.registry,
            &mut ids,
        )
        .expect("choose option");
    assert_eq!(
        parameter(&chosen.upserts, &id("parameter", "story_step")),
        ParameterValue::Counter(7)
    );
    let instance = chosen
        .upserts
        .iter()
        .find_map(|record| match record {
            DomainRecord::EventInstance(value) => Some(value),
            _ => None,
        })
        .expect("event instance");
    assert_eq!(instance.status, EventStatus::Completed);
    assert_eq!(
        instance.committed_options,
        vec![id("event_option", "answer")]
    );
    assert_eq!(
        chosen
            .record_ops()
            .expect("record ops")
            .iter()
            .filter(|op| op.revision == Revision::new(1))
            .count(),
        2
    );
    assert!(matches!(
        world.execute(
            command(
                &fixture,
                Revision::ZERO,
                "6a41",
                WorldCommandKind::ChooseEventOption {
                    event_instance_id: fixture.event_instance,
                    option_id: id("event_option", "answer"),
                },
            ),
            &fixture.registry,
            &mut ids,
        ),
        Err(WorldError::Conflict { .. })
    ));

    let mut unavailable = GameWorld::from_records(
        Revision::ZERO,
        fixture.records.clone(),
        fixture.config.clone(),
        &fixture.registry,
    )
    .expect("second world");
    unavailable
        .execute(
            command(
                &fixture,
                Revision::ZERO,
                "6a42",
                WorldCommandKind::PerformGameplayAction {
                    action_id: id("gameplay_action", "study"),
                    arguments: BTreeMap::from([(
                        id("action_parameter", "effort"),
                        ParameterValue::Counter(1),
                    )]),
                },
            ),
            &fixture.registry,
            &mut ids,
        )
        .expect("lower resource");
    assert!(matches!(
        unavailable.execute(
            command(
                &fixture,
                Revision::new(1),
                "6a43",
                WorldCommandKind::ChooseEventOption {
                    event_instance_id: fixture.event_instance,
                    option_id: id("event_option", "answer"),
                },
            ),
            &fixture.registry,
            &mut ids,
        ),
        Err(WorldError::DomainRule {
            rule: "event_option_not_visible"
        })
    ));
    assert_eq!(unavailable.revision(), Revision::new(1));
    assert!(matches!(
        unavailable.execute(
            command(
                &fixture,
                Revision::new(1),
                "6a44",
                WorldCommandKind::ChooseEventOption {
                    event_instance_id: fixture.event_instance,
                    option_id: id("event_option", "wait"),
                },
            ),
            &fixture.registry,
            &mut ids,
        ),
        Err(WorldError::DomainRule {
            rule: "event_option_not_enabled"
        })
    ));
}

#[test]
fn condition_stacks_and_item_skill_grants_use_persistent_records() {
    let fixture = fixture(RuleLimits::default());
    let mut world = GameWorld::from_records(
        Revision::ZERO,
        fixture.records.clone(),
        fixture.config.clone(),
        &fixture.registry,
    )
    .expect("world");
    let mut ids = SystemIdGenerator;
    for (revision, suffix) in [(Revision::ZERO, "6a50"), (Revision::new(1), "6a51")] {
        world
            .execute(
                command(
                    &fixture,
                    revision,
                    suffix,
                    WorldCommandKind::PerformGameplayAction {
                        action_id: id("gameplay_action", "study"),
                        arguments: BTreeMap::from([(
                            id("action_parameter", "effort"),
                            ParameterValue::Counter(1),
                        )]),
                    },
                ),
                &fixture.registry,
                &mut ids,
            )
            .expect("study");
    }
    let grant = world
        .execute(
            command(
                &fixture,
                Revision::new(2),
                "6a52",
                WorldCommandKind::PerformGameplayAction {
                    action_id: id("gameplay_action", "claim_reward"),
                    arguments: BTreeMap::new(),
                },
            ),
            &fixture.registry,
            &mut ids,
        )
        .expect("grant");
    let records = world.project_records().expect("records");
    let conditions = records
        .iter()
        .filter_map(|record| match record {
            DomainRecord::Condition(value) => Some(value),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(conditions.len(), 1);
    assert_eq!(conditions[0].stacks.get(), 2);
    assert_eq!(conditions[0].expires_at, Some(WorldTime::from_ticks(5)));
    assert!(
        grant
            .events
            .iter()
            .any(|event| matches!(event.kind, WorldEventKind::ItemGranted { quantity: 2, .. }))
    );
    assert!(
        grant
            .events
            .iter()
            .any(|event| matches!(event.kind, WorldEventKind::SkillGranted { .. }))
    );
    assert!(records.iter().any(|record| matches!(
        record,
        DomainRecord::SkillGrant(value) if value.skill_id == id("skill", "insight")
    )));
}

#[test]
fn world_clock_rules_execute_once_for_each_crossed_boundary() {
    let fixture = fixture(RuleLimits::default());
    let mut world = GameWorld::from_records(
        Revision::ZERO,
        fixture.records.clone(),
        fixture.config.clone(),
        &fixture.registry,
    )
    .expect("world");
    let changes = world
        .execute(
            command(
                &fixture,
                Revision::ZERO,
                "6a60",
                WorldCommandKind::AdvanceTime { ticks: 5 },
            ),
            &fixture.registry,
            &mut SystemIdGenerator,
        )
        .expect("advance clock");
    assert_eq!(
        parameter(&changes.upserts, &id("parameter", "story_step")),
        ParameterValue::Counter(5)
    );
    assert_eq!(
        changes
            .events
            .iter()
            .filter(|event| matches!(
                &event.kind,
                WorldEventKind::RuleTriggered { rule_id, .. }
                    if rule_id == &id("rule", "clock_pulse")
            ))
            .count(),
        2
    );
    let state = world
        .project_records()
        .expect("records")
        .into_iter()
        .find_map(|record| match record {
            DomainRecord::RuleState(state) if state.definition_id == id("rule", "clock_pulse") => {
                Some(state)
            }
            _ => None,
        })
        .expect("clock rule state");
    assert_eq!(state.trigger_count, 2);
    assert_eq!(state.last_triggered_at, Some(WorldTime::from_ticks(5)));
}
