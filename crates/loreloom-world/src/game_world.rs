use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroU32,
};

use bevy_ecs::{entity::Entity, prelude::Resource, world::World};
use loreloom_content::{
    Definition, DefinitionRegistry, DurationPolicy, EffectDefinition, ItemDefinition, SkillKind,
    SkillTarget,
};
use loreloom_core::{
    ActionId, ActorId, CharacterController, CharacterLifetime, CharacterRecord, ConditionRecord,
    ContentDefinitionId, DomainRecord, EntityOrigin, EventId, ExecutionChangeSet, Fixed,
    GoalRecord, GoalStatus, IdGenerator, ItemRecord, KnownFactRecord, LifeState, ObjectId,
    PlaceRecord, Posture, RecordKey, Revision, SceneRecord, ShortText, SkillGrantRecord,
    SkillSource, StackState, TranscriptItemRecord, WorldCommand, WorldCommandKind, WorldEvent,
    WorldEventKind, WorldStateRecord,
};

use crate::{
    ObjectKind, PersistentId, WorldError,
    components::{
        CharacterComponent, ConditionComponent, EventInstanceComponent, GoalComponent,
        ItemComponent, KnownFactComponent, ParameterSetComponent, PlaceComponent,
        RelationshipComponent, RuleStateComponent, SceneComponent, SkillGrantComponent,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldConfig {
    pub inventory_root_definition: ContentDefinitionId,
    pub spawn_system_definition: ContentDefinitionId,
}

#[derive(Debug, Clone, PartialEq, Eq, Resource)]
struct WorldStateResource(WorldStateRecord);

pub struct GameWorld {
    world: World,
    revision: Revision,
    objects: BTreeMap<ObjectId, Entity>,
    transcripts: BTreeMap<loreloom_core::TranscriptItemId, TranscriptItemRecord>,
    config: WorldConfig,
}

impl GameWorld {
    pub fn from_records(
        revision: Revision,
        records: impl IntoIterator<Item = DomainRecord>,
        config: WorldConfig,
        registry: &DefinitionRegistry,
    ) -> Result<Self, WorldError> {
        let mut game = Self {
            world: World::new(),
            revision,
            objects: BTreeMap::new(),
            transcripts: BTreeMap::new(),
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
        let revision = self.revision.next()?;
        let action_id = command.action_id;
        let actor_id = command.actor_id;
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
                actor_id, grant_id, target, action_id, revision, registry, ids,
            )?,
            WorldCommandKind::AdvanceTime { ticks } => {
                self.advance_time(actor_id, ticks, action_id, revision, ids)?
            }
            WorldCommandKind::SpawnCharacter { spec } => {
                self.spawn_character(actor_id, *spec, action_id, revision, registry, ids)?
            }
            WorldCommandKind::PromoteCharacter { actor_id: target } => {
                self.promote_character(actor_id, target, action_id, revision, ids)?
            }
            WorldCommandKind::AppendTranscript { items } => {
                self.append_transcripts(actor_id, items, revision)?
            }
        };
        self.revision = revision;
        self.validate(registry)?;
        upserts.sort_by_key(domain_sort_key);
        events.sort_by_key(|event| event.id);
        Ok(ExecutionChangeSet {
            action_id,
            expected_revision: command.expected_revision,
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
                require_definition(registry, &value.0.condition_id, "condition")?;
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
            } else if let Some(value) = self.world.get::<EventInstanceComponent>(*entity)
                && let Some(scene_id) = value.0.scene_id
            {
                self.scene(scene_id)?;
            }
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
        for effect in &definition.effects {
            match effect {
                EffectDefinition::ResourceDelta {
                    resource_id,
                    amount,
                } => {
                    let pool =
                        character
                            .resources
                            .get_mut(resource_id)
                            .ok_or(WorldError::DomainRule {
                                rule: "skill_resource_missing",
                            })?;
                    let next = pool.current.checked_add(*amount)?;
                    if next < Fixed::ZERO || next > pool.base_maximum {
                        return domain_rule("skill_resource_out_of_range");
                    }
                    pool.current = next;
                    events.push(event(
                        ids,
                        action_id,
                        actor_id,
                        revision,
                        WorldEventKind::ResourceChanged {
                            character_id: actor_id,
                            resource_id: resource_id.clone(),
                            delta: *amount,
                        },
                    )?);
                }
                _ => return domain_rule("skill_effect_not_supported_by_mvp_executor"),
            }
        }
        self.world
            .get_mut::<CharacterComponent>(actor_entity)
            .ok_or(WorldError::WrongObjectKind {
                id: actor_id.object_id(),
            })?
            .0 = character.clone();
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
        Ok((
            vec![
                DomainRecord::Character(character),
                DomainRecord::SkillGrant(updated_grant),
            ],
            Vec::new(),
            events,
            summary("skill used")?,
        ))
    }

    fn advance_time(
        &mut self,
        actor_id: ActorId,
        ticks: u64,
        action_id: ActionId,
        revision: Revision,
        ids: &mut impl IdGenerator,
    ) -> Result<ChangeParts, WorldError> {
        if ticks == 0 {
            return domain_rule("clock_advance_zero");
        }
        let from = self.world_state().clock;
        let to = from.checked_add(ticks)?;
        self.world.resource_mut::<WorldStateResource>().0.clock = to;
        let mut deletes = Vec::new();
        let expired = self
            .objects
            .iter()
            .filter_map(|(id, entity)| {
                self.world
                    .get::<ConditionComponent>(*entity)
                    .filter(|condition| condition.0.expires_at.is_some_and(|expiry| expiry <= to))
                    .map(|_| (*id, *entity))
            })
            .collect::<Vec<_>>();
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
        for (id, entity) in expired {
            let record = self.project_entity(entity)?;
            deletes.push(record.key()?);
            self.world.despawn(entity);
            self.objects.remove(&id);
            events.push(event(
                ids,
                action_id,
                actor_id,
                revision,
                WorldEventKind::ConditionExpired { condition_id: id },
            )?);
        }
        Ok((
            vec![DomainRecord::WorldState(self.world_state().clone())],
            deletes,
            events,
            summary("world clock advanced")?,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_character(
        &mut self,
        actor_id: ActorId,
        spec: loreloom_core::CharacterSpawnSpec,
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
        if spec.controller == CharacterController::Agent && spec.agent_binding.is_none() {
            return domain_rule("spawn_agent_binding_missing");
        }
        if spec.inventory.len() > spec.trusted_constraints.maximum_items as usize
            || spec.skills.len() > spec.trusted_constraints.maximum_skills as usize
        {
            return domain_rule("spawn_budget_exceeded");
        }
        let character_id = ActorId::from(ObjectId::generate_with(ids)?);
        let inventory_root_id = ObjectId::generate_with(ids)?;
        let root_entry = registry
            .get(&self.config.inventory_root_definition)
            .ok_or_else(|| WorldError::DefinitionNotFound {
                id: self.config.inventory_root_definition.clone(),
            })?;
        let Definition::Item(root_definition) = &root_entry.definition else {
            return Err(WorldError::DefinitionNotFound {
                id: self.config.inventory_root_definition.clone(),
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
        let mut inventory = spec.inventory.clone();
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
            let entry = registry.get(&item.definition_id).ok_or_else(|| {
                WorldError::DefinitionNotFound {
                    id: item.definition_id.clone(),
                }
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
        let clock = self.world_state().clock;
        let mut conditions = spec.conditions.clone();
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
        let mut skills = spec.skills.clone();
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
                    rule_id: self.config.spawn_system_definition.clone(),
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
        for record in &records {
            record.validate()?;
        }
        for record in records.iter().cloned() {
            self.spawn_record(record)?;
        }
        let event = event(
            ids,
            action_id,
            actor_id,
            revision,
            WorldEventKind::CharacterSpawned { character_id },
        )?;
        Ok((
            records,
            Vec::new(),
            vec![event],
            summary("character spawned")?,
        ))
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
