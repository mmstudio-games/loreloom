use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    num::NonZeroU32,
};

use bevy_ecs::{entity::Entity, prelude::Resource, world::World};
use loreloom_content::{
    CharacterCompileRequest, Definition, DefinitionRegistry, DurationPolicy, EffectDefinition,
    InitialCharacterLifetime, ItemDefinition, ParameterPersistence, ParameterType,
    PredicateDefinition, SceneSpawnPlan, SkillKind, SkillTarget, StackPolicy, TriggerDefinition,
};
use loreloom_core::{
    ActionId, ActorId, CharacterController, CharacterLifetime, CharacterRecord, CharacterSpawnSpec,
    ConditionRecord, ConditionSource, ContentDefinitionId, DomainRecord, EntityOrigin, EventId,
    EventStatus, ExecutionChangeSet, Fixed, GoalRecord, GoalStatus, IdGenerator, IntensityPolicy,
    ItemRecord, KnownFactRecord, LifeState, ObjectId, ParameterSetRecord, ParameterValue,
    PlaceRecord, Posture, RecordKey, Revision, RuleStateRecord, SceneRecord, ShortText,
    SkillGrantRecord, SkillSource, StackState, TranscriptItemRecord, WorldCommand,
    WorldCommandKind, WorldEvent, WorldEventKind, WorldId, WorldStateRecord, WorldTime,
};

use crate::{
    ObjectKind, PersistentId, WorldError,
    components::{
        CharacterComponent, ConditionComponent, EventInstanceComponent, GoalComponent,
        ItemComponent, KnownFactComponent, ParameterSetComponent, PlaceComponent,
        RelationshipComponent, RuleStateComponent, SceneComponent, SkillGrantComponent,
    },
};

mod declarative;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldConfig {
    pub inventory_root_definition: ContentDefinitionId,
    pub spawn_system_definition: ContentDefinitionId,
    pub rule_limits: RuleLimits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleLimits {
    pub max_triggered_rules: u32,
    pub max_evaluated_predicates: u32,
    pub max_applied_effects: u32,
    pub max_cascade_depth: u32,
}

impl Default for RuleLimits {
    fn default() -> Self {
        Self {
            max_triggered_rules: 128,
            max_evaluated_predicates: 1_024,
            max_applied_effects: 512,
            max_cascade_depth: 8,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldBootstrap {
    pub world_id: WorldId,
    pub records: Vec<DomainRecord>,
    pub active_scene: ObjectId,
    pub player_actor: ActorId,
    pub characters: BTreeMap<ShortText, ActorId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Resource)]
struct WorldStateResource(WorldStateRecord);

pub struct GameWorld {
    world: World,
    revision: Revision,
    objects: BTreeMap<ObjectId, Entity>,
    transcripts: BTreeMap<loreloom_core::TranscriptItemId, TranscriptItemRecord>,
    session_parameters: BTreeMap<ContentDefinitionId, ParameterValue>,
    config: WorldConfig,
}

impl GameWorld {
    pub fn bootstrap(
        plan: &SceneSpawnPlan,
        rng_seed: [u8; 32],
        registry: &DefinitionRegistry,
        config: WorldConfig,
        ids: &mut impl IdGenerator,
    ) -> Result<WorldBootstrap, WorldError> {
        let world_id = WorldId::generate_with(ids)?;
        let scene_id = ObjectId::generate_with(ids)?;

        let mut places = plan.places.clone();
        places.sort_by(|left, right| left.definition_id.cmp(&right.definition_id));
        let mut place_ids = BTreeMap::new();
        for place in &places {
            if place_ids
                .insert(place.definition_id.clone(), ObjectId::generate_with(ids)?)
                .is_some()
            {
                return invariant("bootstrap.place_definition_unique");
            }
        }
        let entry_place =
            place_ids
                .get(&plan.entry_place)
                .copied()
                .ok_or(WorldError::Invariant {
                    invariant: "bootstrap.entry_place",
                })?;

        let mut entries = plan.characters.clone();
        entries.sort_by(|left, right| left.local_key.cmp(&right.local_key));
        let mut characters = BTreeMap::new();
        for entry in &entries {
            if characters
                .insert(
                    entry.local_key.clone(),
                    ActorId::from(ObjectId::generate_with(ids)?),
                )
                .is_some()
            {
                return invariant("bootstrap.character_local_key_unique");
            }
        }
        let players = entries
            .iter()
            .filter(|entry| entry.controller == CharacterController::Player)
            .collect::<Vec<_>>();
        if players.len() != 1 {
            return invariant("bootstrap.exactly_one_player");
        }
        let player_actor = characters[&players[0].local_key];

        let mut records = vec![
            DomainRecord::WorldState(WorldStateRecord {
                id: world_id,
                player_actor,
                active_scene: scene_id,
                clock: WorldTime::ZERO,
                rng_seed,
            }),
            DomainRecord::Scene(SceneRecord {
                id: scene_id,
                display_name: plan.display_name.clone(),
                framing: plan.framing.clone(),
                entry_place,
                active: true,
                origin: plan.origin.clone(),
            }),
        ];
        records.extend(places.into_iter().map(|place| {
            DomainRecord::Place(PlaceRecord {
                id: place_ids[&place.definition_id],
                scene_id,
                display_name: place.display_name,
                description: place.description,
                tags: place.tags,
                origin: place.origin,
            })
        }));
        for entry in entries {
            let place_id =
                place_ids
                    .get(&entry.place_id)
                    .copied()
                    .ok_or(WorldError::Invariant {
                        invariant: "bootstrap.character_place",
                    })?;
            let lifetime = match entry.lifetime {
                InitialCharacterLifetime::Scene => CharacterLifetime::Scene { scene_id },
                InitialCharacterLifetime::Persistent => CharacterLifetime::Persistent,
            };
            let spec = registry.compile_character(
                &entry.character_id,
                CharacterCompileRequest {
                    scene_id,
                    place_id,
                    controller: entry.controller,
                    lifetime,
                },
            )?;
            records.extend(materialize_character_records(
                spec,
                characters[&entry.local_key],
                WorldTime::ZERO,
                registry,
                &config,
                ids,
            )?);
        }
        let mut parameter_sets =
            BTreeMap::<ContentDefinitionId, BTreeMap<ContentDefinitionId, ParameterValue>>::new();
        let mut rule_ids = Vec::new();
        for (_, registered) in registry.iter() {
            match &registered.definition {
                Definition::Parameter(parameter)
                    if parameter.persistence == ParameterPersistence::Save =>
                {
                    parameter_sets
                        .entry(registered.origin.pack_id.clone())
                        .or_default()
                        .insert(parameter.id.clone(), parameter.default.clone());
                }
                Definition::Rule(rule) => rule_ids.push(rule.id.clone()),
                _ => {}
            }
        }
        for (pack_id, values) in parameter_sets {
            records.push(DomainRecord::ParameterSet(ParameterSetRecord {
                id: ObjectId::generate_with(ids)?,
                schema_id: pack_id,
                values,
            }));
        }
        rule_ids.sort();
        for definition_id in rule_ids {
            records.push(DomainRecord::RuleState(RuleStateRecord {
                id: ObjectId::generate_with(ids)?,
                definition_id,
                values: BTreeMap::new(),
                trigger_count: 0,
                last_triggered_at: None,
            }));
        }
        records.sort_by_key(domain_sort_key);
        Self::from_records(Revision::ZERO, records.iter().cloned(), config, registry)?;
        Ok(WorldBootstrap {
            world_id,
            records,
            active_scene: scene_id,
            player_actor,
            characters,
        })
    }

    pub fn from_records(
        revision: Revision,
        records: impl IntoIterator<Item = DomainRecord>,
        config: WorldConfig,
        registry: &DefinitionRegistry,
    ) -> Result<Self, WorldError> {
        validate_rule_limits(config.rule_limits)?;
        let mut game = Self {
            world: World::new(),
            revision,
            objects: BTreeMap::new(),
            transcripts: BTreeMap::new(),
            session_parameters: registry
                .iter()
                .filter_map(|(_, registered)| match &registered.definition {
                    Definition::Parameter(parameter)
                        if parameter.persistence == ParameterPersistence::Session =>
                    {
                        Some((parameter.id.clone(), parameter.default.clone()))
                    }
                    _ => None,
                })
                .collect(),
            config,
        };
        for record in records {
            game.insert_initial(record)?;
        }
        if !game.world.contains_resource::<WorldStateResource>() {
            return Err(WorldError::WorldState);
        }
        game.validate(registry)?;
        Ok(game)
    }

    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    #[must_use]
    pub fn world_state(&self) -> &WorldStateRecord {
        &self.world.resource::<WorldStateResource>().0
    }

    #[must_use]
    pub fn character(&self, id: ActorId) -> Option<&CharacterRecord> {
        self.objects
            .get(id.as_object_id())
            .and_then(|entity| self.world.get::<CharacterComponent>(*entity))
            .map(|component| &component.0)
    }

    #[must_use]
    pub fn item(&self, id: ObjectId) -> Option<&ItemRecord> {
        self.objects
            .get(&id)
            .and_then(|entity| self.world.get::<ItemComponent>(*entity))
            .map(|component| &component.0)
    }

    pub fn transcripts(&self) -> impl Iterator<Item = &TranscriptItemRecord> {
        self.transcripts.values()
    }

    #[must_use]
    pub fn session_parameters(&self) -> &BTreeMap<ContentDefinitionId, ParameterValue> {
        &self.session_parameters
    }

    pub fn restore_session_parameters(
        &mut self,
        values: BTreeMap<ContentDefinitionId, ParameterValue>,
        registry: &DefinitionRegistry,
    ) -> Result<(), WorldError> {
        let expected = registry
            .iter()
            .filter_map(|(_, registered)| match &registered.definition {
                Definition::Parameter(definition)
                    if definition.persistence == ParameterPersistence::Session =>
                {
                    Some(definition.id.clone())
                }
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        if values.keys().cloned().collect::<BTreeSet<_>>() != expected {
            return invariant("session_parameter.definition_coverage");
        }
        for (id, value) in &values {
            let Some(Definition::Parameter(definition)) =
                registry.get(id).map(|entry| &entry.definition)
            else {
                return Err(WorldError::DefinitionNotFound { id: id.clone() });
            };
            if definition.persistence != ParameterPersistence::Session {
                return invariant("session_parameter.persistence");
            }
            self.validate_parameter_value(&definition.value_type, value)?;
        }
        self.session_parameters = values;
        Ok(())
    }

    pub fn project_records(&self) -> Result<Vec<DomainRecord>, WorldError> {
        let mut records = vec![DomainRecord::WorldState(self.world_state().clone())];
        for entity in self.objects.values() {
            records.push(self.project_entity(*entity)?);
        }
        records.extend(
            self.transcripts
                .values()
                .cloned()
                .map(DomainRecord::TranscriptItem),
        );
        records.sort_by_key(domain_sort_key);
        Ok(records)
    }

    pub fn execute(
        &mut self,
        command: WorldCommand,
        registry: &DefinitionRegistry,
        ids: &mut impl IdGenerator,
    ) -> Result<ExecutionChangeSet, WorldError> {
        if command.expected_revision != self.revision {
            return Err(WorldError::Conflict {
                expected: command.expected_revision,
                observed: self.revision,
            });
        }
        let rollback_revision = self.revision;
        let rollback_records = self.project_records()?;
        let rollback_session = self.session_parameters.clone();
        match self.execute_inner(command, registry, ids) {
            Ok(changes) => Ok(changes),
            Err(error) => {
                let mut restored = Self::from_records(
                    rollback_revision,
                    rollback_records,
                    self.config.clone(),
                    registry,
                )?;
                restored.restore_session_parameters(rollback_session, registry)?;
                *self = restored;
                Err(error)
            }
        }
    }

    fn execute_inner(
        &mut self,
        command: WorldCommand,
        registry: &DefinitionRegistry,
        ids: &mut impl IdGenerator,
    ) -> Result<ExecutionChangeSet, WorldError> {
        let revision = self.revision.next()?;
        let action_id = command.action_id;
        let actor_id = command.actor_id;
        let expected_revision = command.expected_revision;
        let mut declarative_budget = declarative::ExecutionBudget::default();
        let (mut upserts, deletes, mut events, summary) = match command.kind {
            WorldCommandKind::Move { destination_id } => {
                self.move_character(actor_id, destination_id, action_id, revision, ids)?
            }
            WorldCommandKind::TransferItem {
                item_id,
                container_id,
            } => self.transfer_item(
                actor_id,
                item_id,
                container_id,
                action_id,
                revision,
                registry,
                ids,
            )?,
            WorldCommandKind::EquipItem { item_id, slot_id } => self.equip_item(
                actor_id, item_id, slot_id, action_id, revision, registry, ids,
            )?,
            WorldCommandKind::SplitStack { item_id, quantity } => {
                self.split_stack(actor_id, item_id, quantity, action_id, revision, ids)?
            }
            WorldCommandKind::UseSkill { grant_id, target } => self.use_skill(
                actor_id,
                grant_id,
                target,
                action_id,
                revision,
                registry,
                ids,
                &mut declarative_budget,
            )?,
            WorldCommandKind::AdvanceTime { ticks } => self.advance_time(
                actor_id,
                ticks,
                action_id,
                revision,
                registry,
                ids,
                &mut declarative_budget,
            )?,
            WorldCommandKind::SpawnCharacter { spec } => {
                self.spawn_character(actor_id, *spec, action_id, revision, registry, ids)?
            }
            WorldCommandKind::PromoteCharacter { actor_id: target } => {
                self.promote_character(actor_id, target, action_id, revision, ids)?
            }
            WorldCommandKind::AppendTranscript { items } => {
                self.append_transcripts(actor_id, items, revision)?
            }
            WorldCommandKind::ChooseEventOption {
                event_instance_id,
                option_id,
            } => self.choose_event_option(
                actor_id,
                event_instance_id,
                option_id,
                action_id,
                revision,
                registry,
                ids,
                &mut declarative_budget,
            )?,
            WorldCommandKind::PerformGameplayAction {
                action_id: definition_id,
                arguments,
            } => self.perform_gameplay_action(
                actor_id,
                definition_id,
                arguments,
                action_id,
                revision,
                registry,
                ids,
                &mut declarative_budget,
            )?,
        };
        self.run_declarative_rules(
            actor_id,
            action_id,
            revision,
            registry,
            ids,
            &mut upserts,
            &mut events,
            &mut declarative_budget,
        )?;
        self.revision = revision;
        self.validate(registry)?;
        upserts = coalesce_upserts(upserts)?;
        events.sort_by_key(|event| event.id);
        Ok(ExecutionChangeSet {
            action_id,
            expected_revision,
            revision,
            upserts,
            deletes,
            events,
            safe_summary: summary,
        })
    }

    pub fn validate(&self, registry: &DefinitionRegistry) -> Result<(), WorldError> {
        let state = self.world_state();
        let player = self
            .character(state.player_actor)
            .ok_or(WorldError::WrongObjectKind {
                id: state.player_actor.object_id(),
            })?;
        if player.controller != CharacterController::Player {
            return invariant("world.player_controller");
        }
        let scene = self.scene(state.active_scene)?;
        if !scene.active {
            return invariant("world.active_scene");
        }
        for (id, entity) in &self.objects {
            let persistent = self
                .world
                .get::<PersistentId>(*entity)
                .ok_or(WorldError::DuplicateIdentity)?;
            if persistent.0 != *id {
                return invariant("persistent_id.index");
            }
            self.project_entity(*entity)?.validate()?;
        }
        self.validate_references(registry)
    }

    fn insert_initial(&mut self, record: DomainRecord) -> Result<(), WorldError> {
        record.validate()?;
        match record {
            DomainRecord::WorldState(value) => {
                if self.world.contains_resource::<WorldStateResource>() {
                    return Err(WorldError::WorldState);
                }
                self.world.insert_resource(WorldStateResource(value));
            }
            DomainRecord::TranscriptItem(value) => {
                if self.transcripts.insert(value.id, value).is_some() {
                    return Err(WorldError::DuplicateIdentity);
                }
            }
            value => self.spawn_record(value)?,
        }
        Ok(())
    }

    fn spawn_record(&mut self, record: DomainRecord) -> Result<(), WorldError> {
        let id = object_id(&record).ok_or(WorldError::WorldState)?;
        if self.objects.contains_key(&id) {
            return Err(WorldError::DuplicateIdentity);
        }
        let entity = match record {
            DomainRecord::Scene(value) => self
                .world
                .spawn((PersistentId(id), ObjectKind::Scene, SceneComponent(value)))
                .id(),
            DomainRecord::Place(value) => self
                .world
                .spawn((PersistentId(id), ObjectKind::Place, PlaceComponent(value)))
                .id(),
            DomainRecord::Character(value) => self
                .world
                .spawn((
                    PersistentId(id),
                    ObjectKind::Character,
                    CharacterComponent(value),
                ))
                .id(),
            DomainRecord::Item(value) => self
                .world
                .spawn((PersistentId(id), ObjectKind::Item, ItemComponent(value)))
                .id(),
            DomainRecord::Condition(value) => self
                .world
                .spawn((
                    PersistentId(id),
                    ObjectKind::Condition,
                    ConditionComponent(value),
                ))
                .id(),
            DomainRecord::SkillGrant(value) => self
                .world
                .spawn((
                    PersistentId(id),
                    ObjectKind::SkillGrant,
                    SkillGrantComponent(value),
                ))
                .id(),
            DomainRecord::Relationship(value) => self
                .world
                .spawn((
                    PersistentId(id),
                    ObjectKind::Relationship,
                    RelationshipComponent(value),
                ))
                .id(),
            DomainRecord::KnownFact(value) => self
                .world
                .spawn((
                    PersistentId(id),
                    ObjectKind::KnownFact,
                    KnownFactComponent(value),
                ))
                .id(),
            DomainRecord::Goal(value) => self
                .world
                .spawn((PersistentId(id), ObjectKind::Goal, GoalComponent(value)))
                .id(),
            DomainRecord::EventInstance(value) => self
                .world
                .spawn((
                    PersistentId(id),
                    ObjectKind::EventInstance,
                    EventInstanceComponent(value),
                ))
                .id(),
            DomainRecord::ParameterSet(value) => self
                .world
                .spawn((
                    PersistentId(id),
                    ObjectKind::ParameterSet,
                    ParameterSetComponent(value),
                ))
                .id(),
            DomainRecord::RuleState(value) => self
                .world
                .spawn((
                    PersistentId(id),
                    ObjectKind::RuleState,
                    RuleStateComponent(value),
                ))
                .id(),
            DomainRecord::WorldState(_) | DomainRecord::TranscriptItem(_) => {
                return Err(WorldError::WorldState);
            }
        };
        self.objects.insert(id, entity);
        Ok(())
    }

    fn project_entity(&self, entity: Entity) -> Result<DomainRecord, WorldError> {
        if let Some(value) = self.world.get::<SceneComponent>(entity) {
            return Ok(DomainRecord::Scene(value.0.clone()));
        }
        if let Some(value) = self.world.get::<PlaceComponent>(entity) {
            return Ok(DomainRecord::Place(value.0.clone()));
        }
        if let Some(value) = self.world.get::<CharacterComponent>(entity) {
            return Ok(DomainRecord::Character(value.0.clone()));
        }
        if let Some(value) = self.world.get::<ItemComponent>(entity) {
            return Ok(DomainRecord::Item(value.0.clone()));
        }
        if let Some(value) = self.world.get::<ConditionComponent>(entity) {
            return Ok(DomainRecord::Condition(value.0.clone()));
        }
        if let Some(value) = self.world.get::<SkillGrantComponent>(entity) {
            return Ok(DomainRecord::SkillGrant(value.0.clone()));
        }
        if let Some(value) = self.world.get::<RelationshipComponent>(entity) {
            return Ok(DomainRecord::Relationship(value.0.clone()));
        }
        if let Some(value) = self.world.get::<KnownFactComponent>(entity) {
            return Ok(DomainRecord::KnownFact(value.0.clone()));
        }
        if let Some(value) = self.world.get::<GoalComponent>(entity) {
            return Ok(DomainRecord::Goal(value.0.clone()));
        }
        if let Some(value) = self.world.get::<EventInstanceComponent>(entity) {
            return Ok(DomainRecord::EventInstance(value.0.clone()));
        }
        if let Some(value) = self.world.get::<ParameterSetComponent>(entity) {
            return Ok(DomainRecord::ParameterSet(value.0.clone()));
        }
        if let Some(value) = self.world.get::<RuleStateComponent>(entity) {
            return Ok(DomainRecord::RuleState(value.0.clone()));
        }
        invariant("entity.domain_component")
    }

    fn scene(&self, id: ObjectId) -> Result<&SceneRecord, WorldError> {
        self.objects
            .get(&id)
            .and_then(|entity| self.world.get::<SceneComponent>(*entity))
            .map(|value| &value.0)
            .ok_or(WorldError::WrongObjectKind { id })
    }

    fn place(&self, id: ObjectId) -> Result<&PlaceRecord, WorldError> {
        self.objects
            .get(&id)
            .and_then(|entity| self.world.get::<PlaceComponent>(*entity))
            .map(|value| &value.0)
            .ok_or(WorldError::WrongObjectKind { id })
    }

    fn validate_references(&self, registry: &DefinitionRegistry) -> Result<(), WorldError> {
        let mut parameter_schemas = BTreeSet::new();
        let mut saved_parameters = BTreeSet::new();
        let mut rule_state_counts = BTreeMap::<ContentDefinitionId, u32>::new();
        for entity in self.objects.values() {
            if let Some(value) = self.world.get::<SceneComponent>(*entity) {
                let entry = self.place(value.0.entry_place)?;
                if entry.scene_id != value.0.id {
                    return invariant("scene.entry_place");
                }
            } else if let Some(value) = self.world.get::<PlaceComponent>(*entity) {
                self.scene(value.0.scene_id)?;
            } else if let Some(value) = self.world.get::<CharacterComponent>(*entity) {
                let place = self.place(value.0.location)?;
                let root =
                    self.item(value.0.inventory_root)
                        .ok_or(WorldError::WrongObjectKind {
                            id: value.0.inventory_root,
                        })?;
                if root.container.is_none() || root.owned_by != Some(value.0.id) {
                    return invariant("character.inventory_root");
                }
                if let CharacterLifetime::Scene { scene_id } = value.0.lifetime
                    && place.scene_id != scene_id
                {
                    return invariant("character.scene_lifetime");
                }
                for attribute in value.0.base_attributes.0.keys() {
                    require_definition(registry, attribute, "attribute")?;
                }
                for resource in value.0.resources.keys() {
                    require_definition(registry, resource, "resource")?;
                }
            } else if let Some(value) = self.world.get::<ItemComponent>(*entity) {
                require_definition(registry, &value.0.definition_id, "item")?;
                if let Some(container) = value.0.contained_by {
                    let parent = self
                        .item(container)
                        .ok_or(WorldError::WrongObjectKind { id: container })?;
                    if parent.container.is_none() || container == value.0.id {
                        return invariant("item.containment");
                    }
                }
                if let Some(owner) = value.0.owned_by {
                    self.character(owner).ok_or(WorldError::WrongObjectKind {
                        id: owner.object_id(),
                    })?;
                }
                if let Some(equipped) = &value.0.equipped
                    && value.0.owned_by != Some(equipped.wearer_id)
                {
                    return invariant("item.equipped_owner");
                }
                self.ensure_no_container_cycle(value.0.id, value.0.contained_by)?;
            } else if let Some(value) = self.world.get::<ConditionComponent>(*entity) {
                self.character(value.0.target_id)
                    .ok_or(WorldError::WrongObjectKind {
                        id: value.0.target_id.object_id(),
                    })?;
                let definition = registry
                    .get(&value.0.condition_id)
                    .and_then(|entry| match &entry.definition {
                        Definition::Condition(definition) => Some(definition),
                        _ => None,
                    })
                    .ok_or_else(|| WorldError::DefinitionNotFound {
                        id: value.0.condition_id.clone(),
                    })?;
                let clock = self.world_state().clock;
                if value.0.expires_at.is_some_and(|expiry| expiry <= clock)
                    || value.0.next_periodic_at.is_some_and(|next| next <= clock)
                    || value.0.next_periodic_at.is_some() != definition.periodic.is_some()
                    || value.0.expires_at.is_some()
                        != matches!(definition.duration, DurationPolicy::Finite { .. })
                {
                    return invariant("condition.clock_schedule");
                }
            } else if let Some(value) = self.world.get::<SkillGrantComponent>(*entity) {
                self.character(value.0.owner_id)
                    .ok_or(WorldError::WrongObjectKind {
                        id: value.0.owner_id.object_id(),
                    })?;
                require_definition(registry, &value.0.skill_id, "skill")?;
            } else if let Some(value) = self.world.get::<RelationshipComponent>(*entity) {
                self.require_object(value.0.source_id)?;
                self.require_object(value.0.target_id)?;
            } else if let Some(value) = self.world.get::<KnownFactComponent>(*entity) {
                self.character(value.0.owner_id)
                    .ok_or(WorldError::WrongObjectKind {
                        id: value.0.owner_id.object_id(),
                    })?;
            } else if let Some(value) = self.world.get::<GoalComponent>(*entity) {
                self.character(value.0.owner_id)
                    .ok_or(WorldError::WrongObjectKind {
                        id: value.0.owner_id.object_id(),
                    })?;
            } else if let Some(value) = self.world.get::<EventInstanceComponent>(*entity) {
                if let Some(scene_id) = value.0.scene_id {
                    self.scene(scene_id)?;
                }
                let definition = registry
                    .get(&value.0.definition_id)
                    .and_then(|entry| match &entry.definition {
                        Definition::Event(definition) => Some(definition),
                        _ => None,
                    })
                    .ok_or_else(|| WorldError::DefinitionNotFound {
                        id: value.0.definition_id.clone(),
                    })?;
                if !definition
                    .nodes
                    .iter()
                    .any(|node| node.id == value.0.current_node)
                {
                    return invariant("event_instance.current_node");
                }
            } else if let Some(value) = self.world.get::<ParameterSetComponent>(*entity) {
                if !parameter_schemas.insert(value.0.schema_id.clone()) {
                    return invariant("parameter_set.schema_unique");
                }
                for (parameter_id, parameter_value) in &value.0.values {
                    let entry = registry.get(parameter_id).ok_or_else(|| {
                        WorldError::DefinitionNotFound {
                            id: parameter_id.clone(),
                        }
                    })?;
                    let Definition::Parameter(definition) = &entry.definition else {
                        return Err(WorldError::DefinitionNotFound {
                            id: parameter_id.clone(),
                        });
                    };
                    if definition.id != *parameter_id
                        || definition.persistence != ParameterPersistence::Save
                        || entry.origin.pack_id != value.0.schema_id
                        || !saved_parameters.insert(parameter_id.clone())
                    {
                        return invariant("parameter_set.value_ownership");
                    }
                    self.validate_parameter_value(&definition.value_type, parameter_value)?;
                }
            } else if let Some(value) = self.world.get::<RuleStateComponent>(*entity) {
                require_definition(registry, &value.0.definition_id, "rule")?;
                let count = rule_state_counts
                    .entry(value.0.definition_id.clone())
                    .or_default();
                *count = count.checked_add(1).ok_or(WorldError::Invariant {
                    invariant: "rule_state.count",
                })?;
            }
        }
        let mut expected_saved_parameters = BTreeSet::new();
        let mut expected_session_parameters = BTreeSet::new();
        for (_, entry) in registry.iter() {
            match &entry.definition {
                Definition::Parameter(definition) => match definition.persistence {
                    ParameterPersistence::Save => {
                        expected_saved_parameters.insert(definition.id.clone());
                    }
                    ParameterPersistence::Session => {
                        expected_session_parameters.insert(definition.id.clone());
                    }
                },
                Definition::Rule(definition)
                    if rule_state_counts.get(&definition.id).copied() != Some(1) =>
                {
                    return invariant("rule_state.definition_unique");
                }
                _ => {}
            }
        }
        if saved_parameters != expected_saved_parameters
            || self
                .session_parameters
                .keys()
                .cloned()
                .collect::<BTreeSet<_>>()
                != expected_session_parameters
        {
            return invariant("parameter_set.definition_coverage");
        }
        for (id, value) in &self.session_parameters {
            let Some(Definition::Parameter(definition)) =
                registry.get(id).map(|entry| &entry.definition)
            else {
                return Err(WorldError::DefinitionNotFound { id: id.clone() });
            };
            if definition.persistence != ParameterPersistence::Session {
                return invariant("session_parameter.persistence");
            }
            self.validate_parameter_value(&definition.value_type, value)?;
        }
        Ok(())
    }

    fn require_object(&self, id: ObjectId) -> Result<Entity, WorldError> {
        self.objects
            .get(&id)
            .copied()
            .ok_or(WorldError::ObjectNotFound { id })
    }

    fn ensure_no_container_cycle(
        &self,
        item_id: ObjectId,
        mut parent: Option<ObjectId>,
    ) -> Result<(), WorldError> {
        let mut visited = BTreeSet::new();
        while let Some(id) = parent {
            if id == item_id || !visited.insert(id) {
                return invariant("item.container_cycle");
            }
            parent = self.item(id).and_then(|item| item.contained_by);
        }
        Ok(())
    }

    fn move_character(
        &mut self,
        actor_id: ActorId,
        destination: ObjectId,
        action_id: ActionId,
        revision: Revision,
        ids: &mut impl IdGenerator,
    ) -> Result<ChangeParts, WorldError> {
        self.place(destination)?;
        let current = self
            .character(actor_id)
            .cloned()
            .ok_or(WorldError::WrongObjectKind {
                id: actor_id.object_id(),
            })?;
        let from = current.location;
        if from == destination {
            return domain_rule("already_at_destination");
        }
        if let CharacterLifetime::Scene { scene_id } = current.lifetime
            && self.place(destination)?.scene_id != scene_id
        {
            return domain_rule("scene_lifetime_cannot_leave_scene");
        }
        let entity = self.require_object(actor_id.object_id())?;
        let mut character = self.world.get_mut::<CharacterComponent>(entity).ok_or(
            WorldError::WrongObjectKind {
                id: actor_id.object_id(),
            },
        )?;
        character.0.location = destination;
        let record = DomainRecord::Character(character.0.clone());
        let event = event(
            ids,
            action_id,
            actor_id,
            revision,
            WorldEventKind::CharacterMoved {
                character_id: actor_id,
                from,
                to: destination,
            },
        )?;
        Ok((
            vec![record],
            Vec::new(),
            vec![event],
            summary("character moved")?,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn transfer_item(
        &mut self,
        actor_id: ActorId,
        item_id: ObjectId,
        container_id: ObjectId,
        action_id: ActionId,
        revision: Revision,
        registry: &DefinitionRegistry,
        ids: &mut impl IdGenerator,
    ) -> Result<ChangeParts, WorldError> {
        let item = self
            .item(item_id)
            .cloned()
            .ok_or(WorldError::WrongObjectKind { id: item_id })?;
        if item.owned_by != Some(actor_id) {
            return domain_rule("item_not_owned_by_actor");
        }
        let container = self
            .item(container_id)
            .cloned()
            .ok_or(WorldError::WrongObjectKind { id: container_id })?;
        let capacity = container
            .container
            .ok_or(WorldError::WrongObjectKind { id: container_id })?;
        if container.owned_by.is_some() && container.owned_by != Some(actor_id) {
            return domain_rule("container_not_owned_by_actor");
        }
        self.ensure_no_container_cycle(item_id, Some(container_id))?;
        let direct_children = self
            .items()
            .filter(|candidate| candidate.contained_by == Some(container_id))
            .count();
        if item.contained_by != Some(container_id)
            && direct_children >= capacity.max_children as usize
        {
            return domain_rule("container_child_capacity");
        }
        let existing_weight = self.container_weight(container_id, registry)?;
        let moving_weight = self.item_tree_weight(item_id, registry, &mut BTreeSet::new())?;
        if item.contained_by != Some(container_id)
            && existing_weight.checked_add(moving_weight)? > capacity.max_weight_grams
        {
            return domain_rule("container_weight_capacity");
        }
        let from = item
            .contained_by
            .or(item.located_at)
            .ok_or(WorldError::Invariant {
                invariant: "item.physical_location",
            })?;
        let entity = self.require_object(item_id)?;
        let mut component = self
            .world
            .get_mut::<ItemComponent>(entity)
            .ok_or(WorldError::WrongObjectKind { id: item_id })?;
        component.0.contained_by = Some(container_id);
        component.0.located_at = None;
        let record = DomainRecord::Item(component.0.clone());
        let event = event(
            ids,
            action_id,
            actor_id,
            revision,
            WorldEventKind::ItemTransferred {
                item_id,
                from,
                to: container_id,
            },
        )?;
        Ok((
            vec![record],
            Vec::new(),
            vec![event],
            summary("item transferred")?,
        ))
    }

    fn items(&self) -> impl Iterator<Item = &ItemRecord> {
        self.objects.values().filter_map(|entity| {
            self.world
                .get::<ItemComponent>(*entity)
                .map(|value| &value.0)
        })
    }

    fn container_weight(
        &self,
        container_id: ObjectId,
        registry: &DefinitionRegistry,
    ) -> Result<Fixed, WorldError> {
        let mut total = Fixed::ZERO;
        for item in self
            .items()
            .filter(|item| item.contained_by == Some(container_id))
        {
            total = total.checked_add(self.item_tree_weight(
                item.id,
                registry,
                &mut BTreeSet::new(),
            )?)?;
        }
        Ok(total)
    }

    fn item_tree_weight(
        &self,
        item_id: ObjectId,
        registry: &DefinitionRegistry,
        visited: &mut BTreeSet<ObjectId>,
    ) -> Result<Fixed, WorldError> {
        if !visited.insert(item_id) {
            return invariant("item.container_cycle");
        }
        let item = self
            .item(item_id)
            .ok_or(WorldError::WrongObjectKind { id: item_id })?;
        let definition = item_definition(registry, &item.definition_id)?;
        let quantity = Fixed::from_integer(i64::from(item.stack.0.get()))?;
        let mut total = definition.unit_weight_grams.checked_mul(quantity)?;
        for child in self
            .items()
            .filter(|child| child.contained_by == Some(item_id))
        {
            total = total.checked_add(self.item_tree_weight(child.id, registry, visited)?)?;
        }
        visited.remove(&item_id);
        Ok(total)
    }

    #[allow(clippy::too_many_arguments)]
    fn equip_item(
        &mut self,
        actor_id: ActorId,
        item_id: ObjectId,
        slot_id: ContentDefinitionId,
        action_id: ActionId,
        revision: Revision,
        registry: &DefinitionRegistry,
        ids: &mut impl IdGenerator,
    ) -> Result<ChangeParts, WorldError> {
        let item = self
            .item(item_id)
            .cloned()
            .ok_or(WorldError::WrongObjectKind { id: item_id })?;
        if item.owned_by != Some(actor_id) || item.stack.0.get() != 1 {
            return domain_rule("item_cannot_be_equipped");
        }
        let definition = item_definition(registry, &item.definition_id)?;
        if !definition.equipment_slots.contains(&slot_id) {
            return domain_rule("equipment_slot_not_allowed");
        }
        if self.items().any(|candidate| {
            candidate.id != item_id
                && candidate.equipped.as_ref().is_some_and(|equipped| {
                    equipped.wearer_id == actor_id && equipped.slot_id == slot_id
                })
        }) {
            return domain_rule("equipment_slot_occupied");
        }
        let entity = self.require_object(item_id)?;
        let mut component = self
            .world
            .get_mut::<ItemComponent>(entity)
            .ok_or(WorldError::WrongObjectKind { id: item_id })?;
        component.0.equipped = Some(loreloom_core::EquippedState {
            wearer_id: actor_id,
            slot_id: slot_id.clone(),
        });
        let record = DomainRecord::Item(component.0.clone());
        let event = event(
            ids,
            action_id,
            actor_id,
            revision,
            WorldEventKind::ItemEquipped {
                item_id,
                wearer_id: actor_id,
                slot_id,
            },
        )?;
        Ok((
            vec![record],
            Vec::new(),
            vec![event],
            summary("item equipped")?,
        ))
    }

    fn split_stack(
        &mut self,
        actor_id: ActorId,
        item_id: ObjectId,
        quantity: u32,
        action_id: ActionId,
        revision: Revision,
        ids: &mut impl IdGenerator,
    ) -> Result<ChangeParts, WorldError> {
        let source = self
            .item(item_id)
            .cloned()
            .ok_or(WorldError::WrongObjectKind { id: item_id })?;
        if source.owned_by != Some(actor_id)
            || quantity == 0
            || quantity >= source.stack.0.get()
            || source.container.is_some()
            || source.equipped.is_some()
        {
            return domain_rule("stack_cannot_be_split");
        }
        let new_id = ObjectId::generate_with(ids)?;
        let mut split = source.clone();
        split.id = new_id;
        split.stack = StackState(NonZeroU32::new(quantity).ok_or(WorldError::DomainRule {
            rule: "stack_quantity_zero",
        })?);
        let entity = self.require_object(item_id)?;
        let mut component = self
            .world
            .get_mut::<ItemComponent>(entity)
            .ok_or(WorldError::WrongObjectKind { id: item_id })?;
        component.0.stack = StackState(NonZeroU32::new(source.stack.0.get() - quantity).ok_or(
            WorldError::DomainRule {
                rule: "stack_quantity_zero",
            },
        )?);
        let source_record = DomainRecord::Item(component.0.clone());
        self.spawn_record(DomainRecord::Item(split.clone()))?;
        let event = event(
            ids,
            action_id,
            actor_id,
            revision,
            WorldEventKind::StackSplit {
                source_item_id: item_id,
                new_item_id: new_id,
                quantity,
            },
        )?;
        Ok((
            vec![source_record, DomainRecord::Item(split)],
            Vec::new(),
            vec![event],
            summary("stack split")?,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn use_skill(
        &mut self,
        actor_id: ActorId,
        grant_id: ObjectId,
        target: loreloom_core::SkillTargetRef,
        action_id: ActionId,
        revision: Revision,
        registry: &DefinitionRegistry,
        ids: &mut impl IdGenerator,
        budget: &mut declarative::ExecutionBudget,
    ) -> Result<ChangeParts, WorldError> {
        let entity = self.require_object(grant_id)?;
        let grant = self
            .world
            .get::<SkillGrantComponent>(entity)
            .map(|value| value.0.clone())
            .ok_or(WorldError::WrongObjectKind { id: grant_id })?;
        if grant.owner_id != actor_id || !grant.enabled {
            return domain_rule("skill_grant_unavailable");
        }
        let clock = self.world_state().clock;
        if grant.ready_at.is_some_and(|ready| ready > clock) {
            return domain_rule("skill_on_cooldown");
        }
        let definition = registry
            .get(&grant.skill_id)
            .and_then(|entry| match &entry.definition {
                Definition::Skill(value) => Some(value),
                _ => None,
            })
            .ok_or_else(|| WorldError::DefinitionNotFound {
                id: grant.skill_id.clone(),
            })?;
        if definition.kind != SkillKind::Active {
            return domain_rule("skill_is_not_active");
        }
        validate_skill_target(self, actor_id, &target, &definition.target)?;
        let actor_entity = self.require_object(actor_id.object_id())?;
        let mut character = self
            .world
            .get::<CharacterComponent>(actor_entity)
            .map(|value| value.0.clone())
            .ok_or(WorldError::WrongObjectKind {
                id: actor_id.object_id(),
            })?;
        for cost in &definition.costs {
            let pool =
                character
                    .resources
                    .get_mut(&cost.resource_id)
                    .ok_or(WorldError::DomainRule {
                        rule: "skill_resource_missing",
                    })?;
            if pool.current < cost.amount {
                return domain_rule("skill_resource_insufficient");
            }
            pool.current = pool.current.checked_sub(cost.amount)?;
        }
        let mut events = Vec::new();
        for cost in &definition.costs {
            events.push(event(
                ids,
                action_id,
                actor_id,
                revision,
                WorldEventKind::ResourceChanged {
                    character_id: actor_id,
                    resource_id: cost.resource_id.clone(),
                    delta: Fixed::ZERO.checked_sub(cost.amount)?,
                },
            )?);
        }
        self.world
            .get_mut::<CharacterComponent>(actor_entity)
            .ok_or(WorldError::WrongObjectKind {
                id: actor_id.object_id(),
            })?
            .0 = character.clone();
        let mut upserts = vec![DomainRecord::Character(character)];
        self.apply_effects(
            actor_id,
            &definition.effects,
            &definition.id,
            action_id,
            revision,
            registry,
            ids,
            budget,
            &mut upserts,
            &mut events,
        )?;
        let mut updated_grant = grant.clone();
        updated_grant.ready_at = if definition.cooldown_ticks == 0 {
            None
        } else {
            Some(clock.checked_add(definition.cooldown_ticks)?)
        };
        self.world
            .get_mut::<SkillGrantComponent>(entity)
            .ok_or(WorldError::WrongObjectKind { id: grant_id })?
            .0 = updated_grant.clone();
        events.push(event(
            ids,
            action_id,
            actor_id,
            revision,
            WorldEventKind::SkillUsed {
                grant_id,
                skill_id: grant.skill_id,
                target,
            },
        )?);
        upserts.push(DomainRecord::SkillGrant(updated_grant));
        Ok((upserts, Vec::new(), events, summary("skill used")?))
    }

    #[allow(clippy::too_many_arguments)]
    fn advance_time(
        &mut self,
        actor_id: ActorId,
        ticks: u64,
        action_id: ActionId,
        revision: Revision,
        registry: &DefinitionRegistry,
        ids: &mut impl IdGenerator,
        budget: &mut declarative::ExecutionBudget,
    ) -> Result<ChangeParts, WorldError> {
        if ticks == 0 {
            return domain_rule("clock_advance_zero");
        }
        let from = self.world_state().clock;
        let to = from.checked_add(ticks)?;
        let mut upserts = Vec::new();
        let mut deletes = Vec::new();
        let mut events = vec![event(
            ids,
            action_id,
            actor_id,
            revision,
            WorldEventKind::ClockAdvanced {
                from: from.ticks(),
                to: to.ticks(),
            },
        )?];

        while let Some(boundary) = self.next_condition_boundary(to) {
            self.world.resource_mut::<WorldStateResource>().0.clock = boundary;
            let periodic = self.conditions_periodic_at(boundary);
            for (condition_id, instance_id) in periodic {
                let entity = self.require_object(instance_id)?;
                let condition = self
                    .world
                    .get::<ConditionComponent>(entity)
                    .map(|component| component.0.clone())
                    .ok_or(WorldError::WrongObjectKind { id: instance_id })?;
                if condition.next_periodic_at != Some(boundary) {
                    continue;
                }
                let definition = registry
                    .get(&condition_id)
                    .and_then(|entry| match &entry.definition {
                        Definition::Condition(definition) => Some(definition.clone()),
                        _ => None,
                    })
                    .ok_or_else(|| WorldError::DefinitionNotFound {
                        id: condition_id.clone(),
                    })?;
                let periodic = definition.periodic.ok_or(WorldError::Invariant {
                    invariant: "condition.periodic_definition",
                })?;
                let mut updated = condition;
                updated.next_periodic_at =
                    Some(boundary.checked_add(periodic.interval_ticks.get())?);
                self.world
                    .get_mut::<ConditionComponent>(entity)
                    .ok_or(WorldError::WrongObjectKind { id: instance_id })?
                    .0 = updated.clone();
                upserts.push(DomainRecord::Condition(updated.clone()));
                events.push(event(
                    ids,
                    action_id,
                    updated.target_id,
                    revision,
                    WorldEventKind::ConditionTicked {
                        condition_id: instance_id,
                        scheduled_at: boundary,
                    },
                )?);
                self.apply_effects(
                    updated.target_id,
                    &periodic.effects,
                    &condition_id,
                    action_id,
                    revision,
                    registry,
                    ids,
                    budget,
                    &mut upserts,
                    &mut events,
                )?;
            }

            for (_, instance_id) in self.conditions_expiring_at(boundary) {
                let entity = self.require_object(instance_id)?;
                let condition = self
                    .world
                    .get::<ConditionComponent>(entity)
                    .map(|component| component.0.clone())
                    .ok_or(WorldError::WrongObjectKind { id: instance_id })?;
                if !condition
                    .expires_at
                    .is_some_and(|expiry| expiry <= boundary)
                {
                    continue;
                }
                upserts.retain(|record| object_id(record) != Some(instance_id));
                deletes.push(DomainRecord::Condition(condition.clone()).key()?);
                self.world.despawn(entity);
                self.objects.remove(&instance_id);
                events.push(event(
                    ids,
                    action_id,
                    condition.target_id,
                    revision,
                    WorldEventKind::ConditionExpired {
                        condition_id: instance_id,
                    },
                )?);
            }
        }

        self.world.resource_mut::<WorldStateResource>().0.clock = to;
        upserts.push(DomainRecord::WorldState(self.world_state().clone()));
        Ok((upserts, deletes, events, summary("world clock advanced")?))
    }

    fn next_condition_boundary(&self, to: WorldTime) -> Option<WorldTime> {
        self.objects
            .values()
            .filter_map(|entity| self.world.get::<ConditionComponent>(*entity))
            .flat_map(|condition| {
                [condition.0.next_periodic_at, condition.0.expires_at]
                    .into_iter()
                    .flatten()
            })
            .filter(|boundary| *boundary <= to)
            .min()
    }

    fn conditions_periodic_at(&self, boundary: WorldTime) -> Vec<(ContentDefinitionId, ObjectId)> {
        let mut conditions = self
            .objects
            .iter()
            .filter_map(|(id, entity)| {
                self.world
                    .get::<ConditionComponent>(*entity)
                    .filter(|condition| condition.0.next_periodic_at == Some(boundary))
                    .map(|condition| (condition.0.condition_id.clone(), *id))
            })
            .collect::<Vec<_>>();
        conditions.sort();
        conditions
    }

    fn conditions_expiring_at(&self, boundary: WorldTime) -> Vec<(ContentDefinitionId, ObjectId)> {
        let mut conditions = self
            .objects
            .iter()
            .filter_map(|(id, entity)| {
                self.world
                    .get::<ConditionComponent>(*entity)
                    .filter(|condition| {
                        condition
                            .0
                            .expires_at
                            .is_some_and(|expiry| expiry <= boundary)
                    })
                    .map(|condition| (condition.0.condition_id.clone(), *id))
            })
            .collect::<Vec<_>>();
        conditions.sort();
        conditions
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_character(
        &mut self,
        actor_id: ActorId,
        spec: CharacterSpawnSpec,
        action_id: ActionId,
        revision: Revision,
        registry: &DefinitionRegistry,
        ids: &mut impl IdGenerator,
    ) -> Result<ChangeParts, WorldError> {
        let place = self.place(spec.placement.place_id)?;
        if place.scene_id != spec.placement.scene_id {
            return domain_rule("spawn_place_not_in_scene");
        }
        if let CharacterLifetime::Scene { scene_id } = spec.lifetime
            && scene_id != spec.placement.scene_id
        {
            return domain_rule("spawn_lifetime_scene_mismatch");
        }
        let character_id = ActorId::from(ObjectId::generate_with(ids)?);
        let clock = self.world_state().clock;
        let records =
            materialize_character_records(spec, character_id, clock, registry, &self.config, ids)?;
        let mut candidate_ids = BTreeSet::new();
        for record in &records {
            let id = object_id(record).ok_or(WorldError::WorldState)?;
            if !candidate_ids.insert(id) || self.objects.contains_key(&id) {
                return Err(WorldError::DuplicateIdentity);
            }
        }
        let event = event(
            ids,
            action_id,
            actor_id,
            revision,
            WorldEventKind::CharacterSpawned { character_id },
        )?;
        let safe_summary = summary("character spawned")?;
        for record in records.iter().cloned() {
            self.spawn_record(record)?;
        }
        Ok((records, Vec::new(), vec![event], safe_summary))
    }

    fn promote_character(
        &mut self,
        actor_id: ActorId,
        target: ActorId,
        action_id: ActionId,
        revision: Revision,
        ids: &mut impl IdGenerator,
    ) -> Result<ChangeParts, WorldError> {
        let entity = self.require_object(target.object_id())?;
        let mut component = self.world.get_mut::<CharacterComponent>(entity).ok_or(
            WorldError::WrongObjectKind {
                id: target.object_id(),
            },
        )?;
        if component.0.lifetime == CharacterLifetime::Persistent {
            return domain_rule("character_already_persistent");
        }
        component.0.lifetime = CharacterLifetime::Persistent;
        let record = DomainRecord::Character(component.0.clone());
        let event = event(
            ids,
            action_id,
            actor_id,
            revision,
            WorldEventKind::CharacterPromoted {
                character_id: target,
            },
        )?;
        Ok((
            vec![record],
            Vec::new(),
            vec![event],
            summary("character promoted")?,
        ))
    }

    fn append_transcripts(
        &mut self,
        actor_id: ActorId,
        items: Vec<TranscriptItemRecord>,
        revision: Revision,
    ) -> Result<ChangeParts, WorldError> {
        if items.is_empty() {
            return domain_rule("transcript_items_empty");
        }
        let mut seen = BTreeSet::new();
        for item in &items {
            DomainRecord::TranscriptItem(item.clone()).validate()?;
            if item.revision != Some(revision)
                || !seen.insert(item.id)
                || self.transcripts.contains_key(&item.id)
            {
                return domain_rule("transcript_item_invalid");
            }
            if let loreloom_core::TranscriptSpeaker::Player {
                actor_id: speaker, ..
            } = &item.speaker
                && *speaker != actor_id
            {
                return domain_rule("transcript_player_actor");
            }
        }
        let mut records = items
            .into_iter()
            .map(DomainRecord::TranscriptItem)
            .collect::<Vec<_>>();
        records.sort_by_key(domain_sort_key);
        for record in &records {
            let DomainRecord::TranscriptItem(item) = record else {
                unreachable!("transcript records are constructed above")
            };
            self.transcripts.insert(item.id, item.clone());
        }
        Ok((
            records,
            Vec::new(),
            Vec::new(),
            summary("transcript appended")?,
        ))
    }
}

type ChangeParts = (
    Vec<DomainRecord>,
    Vec<RecordKey>,
    Vec<WorldEvent>,
    ShortText,
);

fn coalesce_upserts(records: Vec<DomainRecord>) -> Result<Vec<DomainRecord>, WorldError> {
    let mut latest = BTreeMap::new();
    for record in records {
        latest.insert(record.key()?, record);
    }
    Ok(latest.into_values().collect())
}

fn validate_rule_limits(limits: RuleLimits) -> Result<(), WorldError> {
    let maximum = RuleLimits::default();
    if limits.max_triggered_rules > maximum.max_triggered_rules
        || limits.max_evaluated_predicates > maximum.max_evaluated_predicates
        || limits.max_applied_effects > maximum.max_applied_effects
        || limits.max_cascade_depth > maximum.max_cascade_depth
    {
        return invariant("rule_limits.maximum");
    }
    Ok(())
}

fn materialize_character_records(
    spec: CharacterSpawnSpec,
    character_id: ActorId,
    clock: WorldTime,
    registry: &DefinitionRegistry,
    config: &WorldConfig,
    ids: &mut impl IdGenerator,
) -> Result<Vec<DomainRecord>, WorldError> {
    if let CharacterLifetime::Scene { scene_id } = spec.lifetime
        && scene_id != spec.placement.scene_id
    {
        return domain_rule("spawn_lifetime_scene_mismatch");
    }
    if (spec.controller == CharacterController::Agent) != spec.agent_binding.is_some() {
        return domain_rule("spawn_agent_binding");
    }
    validate_spawn_constraints(&spec, registry)?;
    for tag in &spec.profile.narrative_tags {
        require_definition(registry, tag, "tag")?;
    }
    for (resource_id, pool) in &spec.resources {
        if resource_id != &pool.resource_id {
            return domain_rule("spawn_resource_key");
        }
        let entry = registry
            .get(resource_id)
            .ok_or_else(|| WorldError::DefinitionNotFound {
                id: resource_id.clone(),
            })?;
        let Definition::Resource(definition) = &entry.definition else {
            return Err(WorldError::DefinitionNotFound {
                id: resource_id.clone(),
            });
        };
        if pool.current < definition.minimum
            || pool.current > pool.base_maximum
            || pool.base_maximum > definition.maximum
        {
            return domain_rule("spawn_resource_range");
        }
    }

    let inventory_root_id = ObjectId::generate_with(ids)?;
    let root_entry = registry
        .get(&config.inventory_root_definition)
        .ok_or_else(|| WorldError::DefinitionNotFound {
            id: config.inventory_root_definition.clone(),
        })?;
    let Definition::Item(root_definition) = &root_entry.definition else {
        return Err(WorldError::DefinitionNotFound {
            id: config.inventory_root_definition.clone(),
        });
    };
    let root_container = root_definition.container.ok_or(WorldError::DomainRule {
        rule: "inventory_root_definition_not_container",
    })?;
    let root = ItemRecord {
        id: inventory_root_id,
        definition_id: root_definition.id.clone(),
        stack: StackState(NonZeroU32::MIN),
        durability: root_definition
            .durability
            .map(|value| loreloom_core::Durability {
                current: value.maximum,
                maximum: value.maximum,
            }),
        container: Some(loreloom_core::ContainerState {
            max_weight_grams: root_container.max_weight_grams,
            max_children: root_container.max_children,
        }),
        contained_by: None,
        owned_by: Some(character_id),
        equipped: None,
        located_at: Some(spec.placement.place_id),
        custom_name: None,
        bound_actor: Some(character_id),
        parameters: BTreeMap::new(),
        instance_adjustments: Vec::new(),
        origin: EntityOrigin::Content {
            origin: root_entry.origin.clone(),
        },
    };
    let character = CharacterRecord {
        id: character_id,
        display_name: spec.display_name.clone(),
        profile: spec.profile.clone(),
        controller: spec.controller,
        lifetime: spec.lifetime,
        location: spec.placement.place_id,
        inventory_root: inventory_root_id,
        agent_binding: spec.agent_binding.clone(),
        base_attributes: spec.attributes.clone(),
        attribute_adjustments: Vec::new(),
        resources: spec.resources.clone(),
        life_state: LifeState::Alive,
        action_state: loreloom_core::ActionState::Idle,
        posture: Posture::Standing,
        origin: spec.origin.clone(),
    };
    let mut records = vec![DomainRecord::Character(character), DomainRecord::Item(root)];

    let mut local_items = BTreeMap::new();
    let mut inventory = spec.inventory;
    inventory.sort_by(|left, right| left.local_key.cmp(&right.local_key));
    for item in &inventory {
        if local_items
            .insert(item.local_key.clone(), ObjectId::generate_with(ids)?)
            .is_some()
        {
            return domain_rule("spawn_duplicate_item_local_key");
        }
    }
    for item in inventory {
        let id = local_items[&item.local_key];
        let parent = match &item.parent_local_key {
            Some(key) => local_items
                .get(key)
                .copied()
                .ok_or(WorldError::DomainRule {
                    rule: "spawn_item_parent_missing",
                })?,
            None => inventory_root_id,
        };
        let entry =
            registry
                .get(&item.definition_id)
                .ok_or_else(|| WorldError::DefinitionNotFound {
                    id: item.definition_id.clone(),
                })?;
        let Definition::Item(definition) = &entry.definition else {
            return Err(WorldError::DefinitionNotFound {
                id: item.definition_id,
            });
        };
        if item.quantity.get() > definition.stack_limit.get()
            || (definition.container.is_some() && item.quantity.get() != 1)
        {
            return domain_rule("spawn_item_stack_invalid");
        }
        records.push(DomainRecord::Item(item_from_definition(
            id,
            parent,
            character_id,
            item.quantity,
            definition,
            &entry.origin,
        )));
    }

    let mut conditions = spec.conditions;
    conditions.sort_by(|left, right| left.condition_id.cmp(&right.condition_id));
    for condition in conditions {
        let entry = registry.get(&condition.condition_id).ok_or_else(|| {
            WorldError::DefinitionNotFound {
                id: condition.condition_id.clone(),
            }
        })?;
        let Definition::Condition(definition) = &entry.definition else {
            return Err(WorldError::DefinitionNotFound {
                id: condition.condition_id,
            });
        };
        let expires_at = match definition.duration {
            DurationPolicy::Permanent => None,
            DurationPolicy::Finite { ticks } => Some(clock.checked_add(ticks.get())?),
        };
        let next_periodic_at = definition
            .periodic
            .as_ref()
            .map(|periodic| clock.checked_add(periodic.interval_ticks.get()))
            .transpose()?;
        records.push(DomainRecord::Condition(ConditionRecord {
            id: ObjectId::generate_with(ids)?,
            target_id: character_id,
            condition_id: definition.id.clone(),
            source: condition.source,
            stacks: condition.stacks,
            intensity: condition.intensity,
            applied_at: clock,
            expires_at,
            next_periodic_at,
            origin: EntityOrigin::Content {
                origin: entry.origin.clone(),
            },
        }));
    }

    let mut skills = spec.skills;
    skills.sort_by(|left, right| left.skill_id.cmp(&right.skill_id));
    for skill in skills {
        let entry =
            registry
                .get(&skill.skill_id)
                .ok_or_else(|| WorldError::DefinitionNotFound {
                    id: skill.skill_id.clone(),
                })?;
        if !matches!(entry.definition, Definition::Skill(_)) {
            return Err(WorldError::DefinitionNotFound { id: skill.skill_id });
        }
        let source = match &spec.origin {
            EntityOrigin::Content { origin } => SkillSource::CharacterDefinition {
                definition_id: origin.definition_id.clone(),
            },
            EntityOrigin::Generated { .. } | EntityOrigin::System { .. } => SkillSource::Rule {
                rule_id: config.spawn_system_definition.clone(),
            },
        };
        records.push(DomainRecord::SkillGrant(SkillGrantRecord {
            id: ObjectId::generate_with(ids)?,
            owner_id: character_id,
            skill_id: skill.skill_id,
            rank: skill.rank,
            proficiency: skill.proficiency,
            source,
            enabled: skill.enabled,
            ready_at: None,
            origin: EntityOrigin::Content {
                origin: entry.origin.clone(),
            },
        }));
    }

    for fact in spec.knowledge {
        require_definition(registry, &fact.predicate_id, "predicate")?;
        records.push(DomainRecord::KnownFact(KnownFactRecord {
            id: ObjectId::generate_with(ids)?,
            owner_id: character_id,
            subject: fact.subject,
            predicate_id: fact.predicate_id,
            value: fact.value,
            status: fact.status,
            confidence: fact.confidence,
            source: fact.source,
            first_known_at: clock,
            last_confirmed_at: clock,
        }));
    }
    for goal in spec.goals {
        records.push(DomainRecord::Goal(GoalRecord {
            id: ObjectId::generate_with(ids)?,
            owner_id: character_id,
            description: goal.description,
            priority: goal.priority,
            status: GoalStatus::Active,
            source: goal.source,
            updated_at: clock,
        }));
    }
    validate_candidate_inventory(&records, registry)?;
    for record in &records {
        record.validate()?;
    }
    Ok(records)
}

fn validate_spawn_constraints(
    spec: &CharacterSpawnSpec,
    registry: &DefinitionRegistry,
) -> Result<(), WorldError> {
    let constraints = &spec.trusted_constraints;
    if spec.inventory.len() > constraints.maximum_items as usize
        || spec.skills.len() > constraints.maximum_skills as usize
    {
        return domain_rule("spawn_budget_exceeded");
    }
    for definition_id in spec
        .conditions
        .iter()
        .map(|value| &value.condition_id)
        .chain(spec.inventory.iter().map(|value| &value.definition_id))
        .chain(spec.skills.iter().map(|value| &value.skill_id))
    {
        if !constraints.allowed_definitions.contains(definition_id) {
            return domain_rule("spawn_definition_not_allowed");
        }
    }
    if constraints.maximum_attribute_points < Fixed::ZERO
        || constraints.minimum_attributes.keys().any(|id| {
            constraints
                .maximum_attributes
                .get(id)
                .is_none_or(|maximum| constraints.minimum_attributes[id] > *maximum)
        })
        || constraints
            .maximum_attributes
            .keys()
            .any(|id| !constraints.minimum_attributes.contains_key(id))
    {
        return domain_rule("spawn_attribute_schema");
    }
    let mut points = Fixed::ZERO;
    for (attribute_id, value) in &spec.attributes.0 {
        require_definition(registry, attribute_id, "attribute")?;
        let minimum = constraints
            .minimum_attributes
            .get(attribute_id)
            .copied()
            .ok_or(WorldError::DomainRule {
                rule: "spawn_attribute_minimum",
            })?;
        let maximum = constraints
            .maximum_attributes
            .get(attribute_id)
            .copied()
            .ok_or(WorldError::DomainRule {
                rule: "spawn_attribute_maximum",
            })?;
        if minimum > maximum || *value < minimum || *value > maximum {
            return domain_rule("spawn_attribute_range");
        }
        points = points.checked_add(value.checked_sub(minimum)?)?;
    }
    if points > constraints.maximum_attribute_points {
        return domain_rule("spawn_attribute_points");
    }
    Ok(())
}

fn validate_candidate_inventory(
    records: &[DomainRecord],
    registry: &DefinitionRegistry,
) -> Result<(), WorldError> {
    let items = records
        .iter()
        .filter_map(|record| match record {
            DomainRecord::Item(item) => Some((item.id, item)),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    for item in items.values() {
        if let Some(parent_id) = item.contained_by {
            let parent = items
                .get(&parent_id)
                .ok_or(WorldError::WrongObjectKind { id: parent_id })?;
            if parent.container.is_none() {
                return domain_rule("spawn_item_parent_not_container");
            }
        }
    }
    for item in items.values() {
        let Some(capacity) = item.container else {
            continue;
        };
        let children = items
            .values()
            .filter(|child| child.contained_by == Some(item.id))
            .collect::<Vec<_>>();
        if children.len() > capacity.max_children as usize {
            return domain_rule("spawn_container_child_capacity");
        }
        let mut weight = Fixed::ZERO;
        for child in children {
            weight = weight.checked_add(candidate_item_tree_weight(
                child.id,
                &items,
                registry,
                &mut BTreeSet::new(),
            )?)?;
        }
        if weight > capacity.max_weight_grams {
            return domain_rule("spawn_container_weight_capacity");
        }
    }
    Ok(())
}

fn candidate_item_tree_weight(
    item_id: ObjectId,
    items: &BTreeMap<ObjectId, &ItemRecord>,
    registry: &DefinitionRegistry,
    visited: &mut BTreeSet<ObjectId>,
) -> Result<Fixed, WorldError> {
    if !visited.insert(item_id) {
        return invariant("spawn_item_container_cycle");
    }
    let item = items
        .get(&item_id)
        .ok_or(WorldError::WrongObjectKind { id: item_id })?;
    let definition = item_definition(registry, &item.definition_id)?;
    let quantity = Fixed::from_integer(i64::from(item.stack.0.get()))?;
    let mut total = definition.unit_weight_grams.checked_mul(quantity)?;
    for child in items
        .values()
        .filter(|child| child.contained_by == Some(item_id))
    {
        total = total.checked_add(candidate_item_tree_weight(
            child.id, items, registry, visited,
        )?)?;
    }
    visited.remove(&item_id);
    Ok(total)
}

fn object_id(record: &DomainRecord) -> Option<ObjectId> {
    match record {
        DomainRecord::Scene(value) => Some(value.id),
        DomainRecord::Place(value) => Some(value.id),
        DomainRecord::Character(value) => Some(value.id.object_id()),
        DomainRecord::Item(value) => Some(value.id),
        DomainRecord::Condition(value) => Some(value.id),
        DomainRecord::SkillGrant(value) => Some(value.id),
        DomainRecord::Relationship(value) => Some(value.id),
        DomainRecord::KnownFact(value) => Some(value.id),
        DomainRecord::Goal(value) => Some(value.id),
        DomainRecord::EventInstance(value) => Some(value.id),
        DomainRecord::ParameterSet(value) => Some(value.id),
        DomainRecord::RuleState(value) => Some(value.id),
        DomainRecord::WorldState(_) | DomainRecord::TranscriptItem(_) => None,
    }
}

fn domain_sort_key(record: &DomainRecord) -> (&'static str, String) {
    let id = match record {
        DomainRecord::WorldState(value) => value.id.to_string(),
        DomainRecord::TranscriptItem(value) => value.id.to_string(),
        _ => object_id(record).map_or_else(String::new, |id| id.to_string()),
    };
    (record.record_type_name(), id)
}

fn require_definition(
    registry: &DefinitionRegistry,
    id: &ContentDefinitionId,
    kind: &'static str,
) -> Result<(), WorldError> {
    if registry
        .get(id)
        .is_some_and(|entry| entry.definition.expected_kind() == kind)
    {
        Ok(())
    } else {
        Err(WorldError::DefinitionNotFound { id: id.clone() })
    }
}

fn item_definition<'a>(
    registry: &'a DefinitionRegistry,
    id: &ContentDefinitionId,
) -> Result<&'a ItemDefinition, WorldError> {
    registry
        .get(id)
        .and_then(|entry| match &entry.definition {
            Definition::Item(value) => Some(value),
            _ => None,
        })
        .ok_or_else(|| WorldError::DefinitionNotFound { id: id.clone() })
}

fn item_from_definition(
    id: ObjectId,
    parent: ObjectId,
    owner: ActorId,
    quantity: NonZeroU32,
    definition: &ItemDefinition,
    origin: &loreloom_core::ContentOrigin,
) -> ItemRecord {
    ItemRecord {
        id,
        definition_id: definition.id.clone(),
        stack: StackState(quantity),
        durability: definition
            .durability
            .map(|value| loreloom_core::Durability {
                current: value.maximum,
                maximum: value.maximum,
            }),
        container: definition
            .container
            .map(|value| loreloom_core::ContainerState {
                max_weight_grams: value.max_weight_grams,
                max_children: value.max_children,
            }),
        contained_by: Some(parent),
        owned_by: Some(owner),
        equipped: None,
        located_at: None,
        custom_name: None,
        bound_actor: None,
        parameters: BTreeMap::new(),
        instance_adjustments: Vec::new(),
        origin: EntityOrigin::Content {
            origin: origin.clone(),
        },
    }
}

fn validate_skill_target(
    world: &GameWorld,
    actor: ActorId,
    target: &loreloom_core::SkillTargetRef,
    schema: &SkillTarget,
) -> Result<(), WorldError> {
    match (schema, target) {
        (SkillTarget::SelfTarget, loreloom_core::SkillTargetRef::SelfTarget) => Ok(()),
        (
            SkillTarget::Character { allow_self, .. },
            loreloom_core::SkillTargetRef::Object { object_id },
        ) => {
            let target_actor = ActorId::from(*object_id);
            world
                .character(target_actor)
                .ok_or(WorldError::WrongObjectKind { id: *object_id })?;
            if !allow_self && target_actor == actor {
                domain_rule("skill_target_self_not_allowed")
            } else {
                Ok(())
            }
        }
        (SkillTarget::Object { .. }, loreloom_core::SkillTargetRef::Object { object_id }) => {
            world.require_object(*object_id)?;
            Ok(())
        }
        (SkillTarget::Place { .. }, loreloom_core::SkillTargetRef::Place { place_id }) => {
            world.place(*place_id)?;
            Ok(())
        }
        _ => domain_rule("skill_target_schema"),
    }
}

fn event(
    ids: &mut impl IdGenerator,
    action_id: ActionId,
    actor_id: ActorId,
    revision: Revision,
    kind: WorldEventKind,
) -> Result<WorldEvent, WorldError> {
    Ok(WorldEvent {
        id: EventId::generate_with(ids)?,
        action_id,
        actor_id,
        revision,
        kind,
    })
}

fn summary(value: &str) -> Result<ShortText, WorldError> {
    ShortText::new(value).map_err(|_| WorldError::Invariant {
        invariant: "safe_summary_bound",
    })
}

fn invariant<T>(invariant: &'static str) -> Result<T, WorldError> {
    Err(WorldError::Invariant { invariant })
}

fn domain_rule<T>(rule: &'static str) -> Result<T, WorldError> {
    Err(WorldError::DomainRule { rule })
}
