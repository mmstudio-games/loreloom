//! P0 evidence for the Armillae/Bevy integration boundary.
//!
//! Every domain type in this file is deliberately test-only. The spike proves
//! that the proposed protocol is expressible without freezing Loreloom's
//! public world API ahead of the Active Spec gate.

use armillae_simulate::{
    BackendId, Clock, ClockDefinition, ClockErrorCode, ClockInstanceId, ClockTransitionError,
    ClockTypeId, ExecuteEntryDefinition, ExecuteEntryId, ExecuteRequest, ExecutionPlane,
    ModuleDescriptor, ModuleId, SIMULATE_API_VERSION, SemanticVersion, Simulation,
    SimulationBuildError, SimulationError, SimulationStatus, SystemDefinition, SystemErrorCode,
    SystemExecutionError, SystemExecutionResult, SystemId, SystemTrigger, TypedAdvanceRequest,
    TypedAdvanceTarget, VersionRequirement,
};
use armillae_simulate_bevy::{
    BEVY_BACKEND_ID, BevyModule, BevyModuleRegistrar, BevySimulation, BevySimulationBuilder,
    ExecuteContext,
};
use bevy_ecs::prelude::{Component, Query, Res, ResMut, Resource, With};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

const MODULE_ID: &str = "games.mmstudio.loreloom.spike/world";
const REST_ENTRY_ID: &str = "games.mmstudio.loreloom.spike/rest";
const REST_SYSTEM_ID: &str = "games.mmstudio.loreloom.spike/system/rest";
const CLOCK_TYPE_ID: &str = "games.mmstudio.loreloom.spike/world-clock";
const REVISION_CONFLICT: &str = "loreloom.world/revision_conflict";

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct LogicalId(String);

impl LogicalId {
    fn new(value: &str) -> Self {
        Self(value.to_owned())
    }
}

#[derive(Component)]
struct Character {
    id: LogicalId,
    name: String,
}

#[derive(Component)]
struct Player;

#[derive(Component)]
struct Attributes {
    strength: i32,
}

#[derive(Clone)]
struct AttributeModifier {
    attribute: String,
    amount: i32,
    source: LogicalId,
}

#[derive(Component)]
struct AttributeModifiers(Vec<AttributeModifier>);

#[derive(Component)]
struct SkillGrants(Vec<LogicalId>);

#[derive(Component)]
struct Vitals {
    health: i32,
    max_health: i32,
    stamina: i32,
    max_stamina: i32,
}

#[derive(Component)]
struct Conditions(Vec<LogicalId>);

#[derive(Component)]
struct Container {
    id: LogicalId,
    owner: LogicalId,
}

#[derive(Component)]
struct Item {
    id: LogicalId,
    name: String,
    contained_by: LogicalId,
}

#[derive(Resource)]
struct WorldRevision(u64);

#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RestInput {
    expected_revision: u64,
    stamina_delta: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
enum CommandOutcome {
    Applied {
        revision: u64,
        stamina: i32,
    },
    Rejected {
        code: String,
        expected_revision: u64,
        actual_revision: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
struct WorldClock {
    tick: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
struct WorldClockStep {
    ticks: u64,
}

impl Clock for WorldClock {
    type Step = WorldClockStep;

    fn advance(&self, step: &Self::Step) -> Result<Self, ClockTransitionError> {
        self.tick
            .checked_add(step.ticks)
            .map(|tick| Self { tick })
            .ok_or_else(|| ClockTransitionError {
                code: ClockErrorCode::new("loreloom.world/clock_overflow")
                    .expect("hard-coded clock error code is valid"),
                message: "world clock overflow".to_owned(),
            })
    }
}

fn module_id() -> ModuleId {
    ModuleId::new(MODULE_ID).expect("hard-coded module ID is valid")
}

fn rest_entry_id() -> ExecuteEntryId {
    ExecuteEntryId::new(REST_ENTRY_ID).expect("hard-coded execute entry ID is valid")
}

fn rest_system_id() -> SystemId {
    SystemId::new(REST_SYSTEM_ID).expect("hard-coded system ID is valid")
}

fn clock_type_id() -> ClockTypeId {
    ClockTypeId::new(CLOCK_TYPE_ID).expect("hard-coded clock type ID is valid")
}

fn system_error(code: &str, message: &str) -> SystemExecutionError {
    SystemExecutionError {
        code: SystemErrorCode::new(code).expect("hard-coded system error code is valid"),
        message: message.to_owned(),
    }
}

fn rest(
    context: Res<ExecuteContext>,
    mut revision: ResMut<WorldRevision>,
    mut player: Query<&mut Vitals, With<Player>>,
) -> SystemExecutionResult {
    let input: RestInput = context.decode().map_err(|_| {
        system_error(
            "loreloom.world/input_decode",
            "validated rest input could not be decoded",
        )
    })?;

    if input.expected_revision != revision.0 {
        return context
            .set_output(&CommandOutcome::Rejected {
                code: REVISION_CONFLICT.to_owned(),
                expected_revision: input.expected_revision,
                actual_revision: revision.0,
            })
            .map_err(Into::into);
    }

    let mut vitals = player.single_mut().map_err(|_| {
        system_error(
            "loreloom.world/player_cardinality",
            "world must contain exactly one player",
        )
    })?;
    let stamina = vitals
        .stamina
        .checked_add(input.stamina_delta)
        .ok_or_else(|| system_error("loreloom.world/resource_overflow", "stamina overflow"))?
        .clamp(0, vitals.max_stamina);
    let next_revision = revision
        .0
        .checked_add(1)
        .ok_or_else(|| system_error("loreloom.world/revision_overflow", "revision overflow"))?;

    vitals.stamina = stamina;
    revision.0 = next_revision;
    context
        .set_output(&CommandOutcome::Applied {
            revision: next_revision,
            stamina,
        })
        .map_err(Into::into)
}

struct LoreloomSpikeModule;

impl BevyModule for LoreloomSpikeModule {
    fn descriptor(&self) -> ModuleDescriptor {
        ModuleDescriptor {
            api_version: SIMULATE_API_VERSION.to_owned(),
            id: module_id(),
            version: SemanticVersion::new("0.1.0").expect("hard-coded version is valid"),
            dependencies: Vec::new(),
            execution: ExecutionPlane::Native {
                backend: BackendId::new(BEVY_BACKEND_ID).expect("hard-coded backend ID is valid"),
                adapter: VersionRequirement::new("=0.1.0-alpha.0")
                    .expect("hard-coded adapter requirement is valid"),
            },
            required_capabilities: Default::default(),
            execute_entries: vec![ExecuteEntryDefinition::for_input_output::<
                RestInput,
                CommandOutcome,
            >(rest_entry_id())],
            clocks: vec![ClockDefinition::for_clock::<WorldClock>(clock_type_id())],
            systems: vec![SystemDefinition {
                id: rest_system_id(),
                trigger: SystemTrigger::Execute {
                    entry: rest_entry_id(),
                },
                before: Vec::new(),
                after: Vec::new(),
            }],
        }
    }

    fn register(
        self: Box<Self>,
        registrar: &mut BevyModuleRegistrar<'_>,
    ) -> Result<(), SimulationBuildError> {
        registrar.bind_clock::<WorldClock>(&clock_type_id())?;
        registrar.add_fallible_system(&rest_system_id(), rest)
    }
}

fn activate() -> BevySimulation {
    let mut builder = BevySimulationBuilder::new();
    builder
        .register_module(LoreloomSpikeModule)
        .expect("test module registers");
    builder.activate().expect("test module activates")
}

fn seed_world(simulation: &mut BevySimulation) {
    simulation
        .write_world(|world| {
            world.insert_resource(WorldRevision(0));
            world.spawn((
                Character {
                    id: LogicalId::new("character/player"),
                    name: "Aster".to_owned(),
                },
                Player,
                Attributes { strength: 8 },
                AttributeModifiers(vec![AttributeModifier {
                    attribute: "strength".to_owned(),
                    amount: 2,
                    source: LogicalId::new("condition/inspired"),
                }]),
                SkillGrants(vec![LogicalId::new("skill/foraging")]),
                Vitals {
                    health: 17,
                    max_health: 20,
                    stamina: 4,
                    max_stamina: 12,
                },
                Conditions(vec![LogicalId::new("condition/inspired")]),
            ));
            world.spawn(Container {
                id: LogicalId::new("container/player-pack"),
                owner: LogicalId::new("character/player"),
            });
            world.spawn(Item {
                id: LogicalId::new("item/field-knife"),
                name: "Field knife".to_owned(),
                contained_by: LogicalId::new("container/player-pack"),
            });
        })
        .expect("test world is seeded");
}

#[derive(Debug, PartialEq, Eq)]
struct CharacterObservation {
    revision: u64,
    id: String,
    name: String,
    effective_strength: i32,
    modifier_sources: Vec<String>,
    skills: Vec<String>,
    health: i32,
    max_health: i32,
    stamina: i32,
    max_stamina: i32,
    conditions: Vec<String>,
    inventory: Vec<(String, String)>,
}

fn observe_player(simulation: &BevySimulation) -> CharacterObservation {
    simulation
        .inspect_world(|world| {
            let revision = world.resource::<WorldRevision>().0;
            let player = world
                .iter_entities()
                .find(|entity| entity.contains::<Player>())
                .expect("player exists");
            let character = player.get::<Character>().expect("player has identity");
            let attributes = player.get::<Attributes>().expect("player has attributes");
            let modifiers = player
                .get::<AttributeModifiers>()
                .expect("player has attribute modifiers");
            let skills = player.get::<SkillGrants>().expect("player has skills");
            let vitals = player.get::<Vitals>().expect("player has resources");
            let conditions = player.get::<Conditions>().expect("player has conditions");
            let container = world
                .iter_entities()
                .filter_map(|entity| entity.get::<Container>())
                .find(|container| container.owner == character.id)
                .expect("player container exists");

            let mut inventory = world
                .iter_entities()
                .filter_map(|entity| entity.get::<Item>())
                .filter(|item| item.contained_by == container.id)
                .map(|item| (item.id.0.clone(), item.name.clone()))
                .collect::<Vec<_>>();
            inventory.sort();
            let mut modifier_sources = modifiers
                .0
                .iter()
                .filter(|modifier| modifier.attribute == "strength")
                .map(|modifier| modifier.source.0.clone())
                .collect::<Vec<_>>();
            modifier_sources.sort();
            let mut skill_ids = skills
                .0
                .iter()
                .map(|skill| skill.0.clone())
                .collect::<Vec<_>>();
            skill_ids.sort();
            let mut condition_ids = conditions
                .0
                .iter()
                .map(|condition| condition.0.clone())
                .collect::<Vec<_>>();
            condition_ids.sort();
            let effective_strength = attributes.strength
                + modifiers
                    .0
                    .iter()
                    .filter(|modifier| modifier.attribute == "strength")
                    .map(|modifier| modifier.amount)
                    .sum::<i32>();

            CharacterObservation {
                revision,
                id: character.id.0.clone(),
                name: character.name.clone(),
                effective_strength,
                modifier_sources,
                skills: skill_ids,
                health: vitals.health,
                max_health: vitals.max_health,
                stamina: vitals.stamina,
                max_stamina: vitals.max_stamina,
                conditions: condition_ids,
                inventory,
            }
        })
        .expect("player observation is projected")
}

#[test]
fn typed_command_uses_revision_cas_and_owned_observations() {
    let mut simulation = activate();
    seed_world(&mut simulation);

    let before = observe_player(&simulation);
    assert_eq!(
        before,
        CharacterObservation {
            revision: 0,
            id: "character/player".to_owned(),
            name: "Aster".to_owned(),
            effective_strength: 10,
            modifier_sources: vec!["condition/inspired".to_owned()],
            skills: vec!["skill/foraging".to_owned()],
            health: 17,
            max_health: 20,
            stamina: 4,
            max_stamina: 12,
            conditions: vec!["condition/inspired".to_owned()],
            inventory: vec![("item/field-knife".to_owned(), "Field knife".to_owned())],
        }
    );

    let applied = simulation
        .execute(ExecuteRequest {
            entry: rest_entry_id(),
            input: json!({ "expected_revision": 0, "stamina_delta": 5 }),
        })
        .expect("matching revision applies");
    assert_eq!(
        applied.output,
        Some(json!({ "status": "applied", "revision": 1, "stamina": 9 }))
    );

    let rejected = simulation
        .execute(ExecuteRequest {
            entry: rest_entry_id(),
            input: json!({ "expected_revision": 0, "stamina_delta": 3 }),
        })
        .expect("business conflict is a structured non-faulting rejection");
    assert_eq!(
        rejected.output,
        Some(json!({
            "status": "rejected",
            "code": REVISION_CONFLICT,
            "expected_revision": 0,
            "actual_revision": 1
        }))
    );
    assert_eq!(simulation.status(), SimulationStatus::Active);

    let after = observe_player(&simulation);
    assert_eq!(after.revision, 1);
    assert_eq!(after.stamina, 9);
}

#[test]
fn json_schema_rejects_invalid_commands_before_world_execution() {
    let mut simulation = activate();
    seed_world(&mut simulation);

    let error = simulation
        .execute(ExecuteRequest {
            entry: rest_entry_id(),
            input: json!({ "stamina_delta": "five", "unexpected": true }),
        })
        .expect_err("invalid JSON input is rejected");
    assert!(matches!(error, SimulationError::InvalidExecuteInput { .. }));
    assert_eq!(observe_player(&simulation).revision, 0);
    assert_eq!(simulation.status(), SimulationStatus::Active);
}

#[test]
fn world_clock_advances_without_exposing_ecs_entities() {
    let mut simulation = activate();
    let clock = ClockInstanceId::new("world/main").expect("hard-coded clock instance ID is valid");
    simulation
        .insert_clock_typed(clock.clone(), WorldClock { tick: 41 })
        .expect("clock is inserted");

    let outcome = simulation
        .advance_typed::<WorldClock>(TypedAdvanceRequest {
            targets: vec![TypedAdvanceTarget {
                instance: clock.clone(),
                step: WorldClockStep { ticks: 1 },
            }],
        })
        .expect("clock advances");
    assert_eq!(outcome.transitions.len(), 1);
    assert_eq!(outcome.transitions[0].after, WorldClock { tick: 42 });
    assert_eq!(
        simulation
            .read_clock_typed::<WorldClock>(&clock)
            .expect("clock can be read"),
        WorldClock { tick: 42 }
    );
}
