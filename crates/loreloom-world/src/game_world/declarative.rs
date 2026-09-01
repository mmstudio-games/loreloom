use super::*;

#[derive(Debug, Default)]
pub(super) struct ExecutionBudget {
    triggered_rules: u32,
    evaluated_predicates: u32,
    applied_effects: u32,
}

impl ExecutionBudget {
    fn charge_rule(&mut self, limits: RuleLimits) -> Result<(), WorldError> {
        self.triggered_rules =
            self.triggered_rules
                .checked_add(1)
                .ok_or(WorldError::DomainRule {
                    rule: "rule_trigger_budget",
                })?;
        if self.triggered_rules > limits.max_triggered_rules {
            return domain_rule("rule_trigger_budget");
        }
        Ok(())
    }

    fn charge_predicate(&mut self, limits: RuleLimits) -> Result<(), WorldError> {
        self.evaluated_predicates =
            self.evaluated_predicates
                .checked_add(1)
                .ok_or(WorldError::DomainRule {
                    rule: "rule_predicate_budget",
                })?;
        if self.evaluated_predicates > limits.max_evaluated_predicates {
            return domain_rule("rule_predicate_budget");
        }
        Ok(())
    }

    fn charge_effect(&mut self, limits: RuleLimits) -> Result<(), WorldError> {
        self.applied_effects =
            self.applied_effects
                .checked_add(1)
                .ok_or(WorldError::DomainRule {
                    rule: "rule_effect_budget",
                })?;
        if self.applied_effects > limits.max_applied_effects {
            return domain_rule("rule_effect_budget");
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
enum RuleSignal {
    WorldEvent {
        event_type: ShortText,
        depth: u32,
    },
    GameplayAction {
        action_id: ContentDefinitionId,
        depth: u32,
    },
    WorldClock {
        from: u64,
        to: u64,
        depth: u32,
    },
    SceneEntered {
        scene_definition_id: ContentDefinitionId,
        depth: u32,
    },
    SceneLeft {
        scene_definition_id: ContentDefinitionId,
        depth: u32,
    },
}

impl RuleSignal {
    const fn depth(&self) -> u32 {
        match self {
            Self::WorldEvent { depth, .. }
            | Self::GameplayAction { depth, .. }
            | Self::WorldClock { depth, .. }
            | Self::SceneEntered { depth, .. }
            | Self::SceneLeft { depth, .. } => *depth,
        }
    }

    fn label(&self) -> Result<ShortText, WorldError> {
        let value = match self {
            Self::WorldEvent { event_type, .. } => format!("world_event:{}", event_type.as_str()),
            Self::GameplayAction { action_id, .. } => format!("gameplay_action:{action_id}"),
            Self::WorldClock { from, to, .. } => format!("world_clock:{from}-{to}"),
            Self::SceneEntered {
                scene_definition_id,
                ..
            } => format!("scene_entered:{scene_definition_id}"),
            Self::SceneLeft {
                scene_definition_id,
                ..
            } => format!("scene_left:{scene_definition_id}"),
        };
        ShortText::new(value).map_err(|_| WorldError::Invariant {
            invariant: "rule_trigger_label_bound",
        })
    }
}

impl GameWorld {
    pub(super) fn validate_parameter_value(
        &self,
        value_type: &ParameterType,
        value: &ParameterValue,
    ) -> Result<(), WorldError> {
        match (value_type, value) {
            (ParameterType::Bool, ParameterValue::Bool(_)) => Ok(()),
            (ParameterType::Fixed { minimum, maximum }, ParameterValue::Fixed(value))
                if minimum <= value && value <= maximum =>
            {
                Ok(())
            }
            (ParameterType::Counter { minimum, maximum }, ParameterValue::Counter(value))
                if minimum <= value && value <= maximum =>
            {
                Ok(())
            }
            (ParameterType::Enum { variants }, ParameterValue::Enum(value))
                if variants.contains(value) =>
            {
                Ok(())
            }
            (ParameterType::TagSet { allowed, maximum }, ParameterValue::TagSet(values))
                if values.len() <= *maximum as usize && values.is_subset(allowed) =>
            {
                Ok(())
            }
            (ParameterType::ObjectRef { allowed_kinds }, ParameterValue::ObjectRef(id)) => {
                let entity = self.require_object(*id)?;
                let kind = self
                    .world
                    .get::<ObjectKind>(entity)
                    .ok_or(WorldError::WrongObjectKind { id: *id })?;
                if allowed_kinds
                    .iter()
                    .any(|allowed| allowed.as_str() == kind.as_str())
                {
                    Ok(())
                } else {
                    domain_rule("parameter_object_kind")
                }
            }
            _ => domain_rule("parameter_value_schema"),
        }
    }

    fn evaluate_predicates(
        &self,
        actor_id: ActorId,
        predicates: &[PredicateDefinition],
        budget: &mut ExecutionBudget,
    ) -> Result<bool, WorldError> {
        for predicate in predicates {
            if !self.evaluate_predicate(actor_id, predicate, budget)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn evaluate_predicate(
        &self,
        actor_id: ActorId,
        predicate: &PredicateDefinition,
        budget: &mut ExecutionBudget,
    ) -> Result<bool, WorldError> {
        budget.charge_predicate(self.config.rule_limits)?;
        match predicate {
            PredicateDefinition::ResourceAtLeast {
                resource_id,
                amount,
            } => Ok(self
                .character(actor_id)
                .ok_or(WorldError::WrongObjectKind {
                    id: actor_id.object_id(),
                })?
                .resources
                .get(resource_id)
                .is_some_and(|pool| pool.current >= *amount)),
            PredicateDefinition::HasCondition { condition_id } => {
                Ok(self.objects.values().any(|entity| {
                    self.world
                        .get::<ConditionComponent>(*entity)
                        .is_some_and(|condition| {
                            condition.0.target_id == actor_id
                                && condition.0.condition_id == *condition_id
                        })
                }))
            }
            PredicateDefinition::HasTag { tag_id } => {
                let actor = self
                    .character(actor_id)
                    .ok_or(WorldError::WrongObjectKind {
                        id: actor_id.object_id(),
                    })?;
                Ok(actor.profile.narrative_tags.contains(tag_id)
                    || self.place(actor.location)?.tags.contains(tag_id))
            }
            PredicateDefinition::Not { predicate } => {
                Ok(!self.evaluate_predicate(actor_id, predicate, budget)?)
            }
            PredicateDefinition::All { predicates } => {
                self.evaluate_predicates(actor_id, predicates, budget)
            }
            PredicateDefinition::Any { predicates } => {
                for predicate in predicates {
                    if self.evaluate_predicate(actor_id, predicate, budget)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn choose_event_option(
        &mut self,
        actor_id: ActorId,
        event_instance_id: ObjectId,
        option_id: ContentDefinitionId,
        action_id: ActionId,
        revision: Revision,
        registry: &DefinitionRegistry,
        ids: &mut impl IdGenerator,
        budget: &mut ExecutionBudget,
    ) -> Result<ChangeParts, WorldError> {
        self.character(actor_id)
            .ok_or(WorldError::WrongObjectKind {
                id: actor_id.object_id(),
            })?;
        let entity = self.require_object(event_instance_id)?;
        let instance = self
            .world
            .get::<EventInstanceComponent>(entity)
            .map(|value| value.0.clone())
            .ok_or(WorldError::WrongObjectKind {
                id: event_instance_id,
            })?;
        if instance.status != EventStatus::Active {
            return domain_rule("event_instance_not_active");
        }
        if let Some(scene_id) = instance.scene_id {
            let actor = self
                .character(actor_id)
                .ok_or(WorldError::WrongObjectKind {
                    id: actor_id.object_id(),
                })?;
            if self.place(actor.location)?.scene_id != scene_id
                || self.world_state().active_scene != scene_id
                || !self.scene(scene_id)?.active
            {
                return domain_rule("event_scene_not_reachable");
            }
        }
        let definition = registry
            .get(&instance.definition_id)
            .and_then(|entry| match &entry.definition {
                Definition::Event(value) => Some(value.clone()),
                _ => None,
            })
            .ok_or_else(|| WorldError::DefinitionNotFound {
                id: instance.definition_id.clone(),
            })?;
        let node = definition
            .nodes
            .iter()
            .find(|node| node.id == instance.current_node)
            .ok_or(WorldError::DomainRule {
                rule: "event_current_node_missing",
            })?;
        let option = node
            .options
            .iter()
            .find(|option| option.id == option_id)
            .cloned()
            .ok_or(WorldError::DomainRule {
                rule: "event_option_not_current",
            })?;
        if !self.evaluate_predicates(actor_id, &option.visible_if, budget)? {
            return domain_rule("event_option_not_visible");
        }
        if !self.evaluate_predicates(actor_id, &option.enabled_if, budget)? {
            return domain_rule("event_option_not_enabled");
        }

        let mut upserts = Vec::new();
        let mut events = Vec::new();
        self.apply_effects(
            actor_id,
            &option.effects,
            &definition.id,
            action_id,
            revision,
            registry,
            ids,
            budget,
            &mut upserts,
            &mut events,
        )?;
        let mut updated = instance;
        updated.committed_options.push(option.id.clone());
        match option.next_node {
            Some(next) => updated.current_node = next,
            None => updated.status = EventStatus::Completed,
        }
        self.world
            .get_mut::<EventInstanceComponent>(entity)
            .ok_or(WorldError::WrongObjectKind {
                id: event_instance_id,
            })?
            .0 = updated.clone();
        upserts.push(DomainRecord::EventInstance(updated));
        events.push(event(
            ids,
            action_id,
            actor_id,
            revision,
            WorldEventKind::EventOptionChosen {
                event_instance_id,
                option_id,
            },
        )?);
        Ok((upserts, Vec::new(), events, summary("event option chosen")?))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn perform_gameplay_action(
        &mut self,
        actor_id: ActorId,
        definition_id: ContentDefinitionId,
        arguments: BTreeMap<ContentDefinitionId, ParameterValue>,
        action_id: ActionId,
        revision: Revision,
        registry: &DefinitionRegistry,
        ids: &mut impl IdGenerator,
        budget: &mut ExecutionBudget,
    ) -> Result<ChangeParts, WorldError> {
        self.character(actor_id)
            .ok_or(WorldError::WrongObjectKind {
                id: actor_id.object_id(),
            })?;
        let definition = registry
            .get(&definition_id)
            .and_then(|entry| match &entry.definition {
                Definition::GameplayAction(value) => Some(value.clone()),
                _ => None,
            })
            .ok_or_else(|| WorldError::DefinitionNotFound {
                id: definition_id.clone(),
            })?;
        let parameters = definition
            .parameters
            .iter()
            .map(|parameter| (&parameter.id, parameter))
            .collect::<BTreeMap<_, _>>();
        if arguments.keys().any(|id| !parameters.contains_key(id)) {
            return domain_rule("gameplay_action_extra_argument");
        }
        if definition
            .parameters
            .iter()
            .any(|parameter| parameter.required && !arguments.contains_key(&parameter.id))
        {
            return domain_rule("gameplay_action_required_argument");
        }
        for (id, value) in &arguments {
            self.validate_parameter_value(&parameters[id].value_type, value)?;
        }
        if !self.evaluate_predicates(actor_id, &definition.predicates, budget)? {
            return domain_rule("gameplay_action_predicate");
        }
        let mut upserts = Vec::new();
        let mut events = Vec::new();
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
        events.push(event(
            ids,
            action_id,
            actor_id,
            revision,
            WorldEventKind::GameplayActionPerformed {
                action_id: definition.id,
            },
        )?);
        Ok((
            upserts,
            Vec::new(),
            events,
            summary("gameplay action performed")?,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn apply_effects(
        &mut self,
        actor_id: ActorId,
        effects: &[EffectDefinition],
        source_definition_id: &ContentDefinitionId,
        action_id: ActionId,
        revision: Revision,
        registry: &DefinitionRegistry,
        ids: &mut impl IdGenerator,
        budget: &mut ExecutionBudget,
        upserts: &mut Vec<DomainRecord>,
        events: &mut Vec<WorldEvent>,
    ) -> Result<(), WorldError> {
        for effect in effects {
            budget.charge_effect(self.config.rule_limits)?;
            self.apply_effect(
                actor_id,
                effect,
                source_definition_id,
                action_id,
                revision,
                registry,
                ids,
                upserts,
                events,
            )?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_effect(
        &mut self,
        actor_id: ActorId,
        effect: &EffectDefinition,
        source_definition_id: &ContentDefinitionId,
        action_id: ActionId,
        revision: Revision,
        registry: &DefinitionRegistry,
        ids: &mut impl IdGenerator,
        upserts: &mut Vec<DomainRecord>,
        events: &mut Vec<WorldEvent>,
    ) -> Result<(), WorldError> {
        match effect {
            EffectDefinition::ResourceDelta {
                resource_id,
                amount,
            } => {
                let resource = registry
                    .get(resource_id)
                    .and_then(|entry| match &entry.definition {
                        Definition::Resource(value) => Some(value),
                        _ => None,
                    })
                    .ok_or_else(|| WorldError::DefinitionNotFound {
                        id: resource_id.clone(),
                    })?;
                let entity = self.require_object(actor_id.object_id())?;
                let mut character = self
                    .world
                    .get::<CharacterComponent>(entity)
                    .map(|value| value.0.clone())
                    .ok_or(WorldError::WrongObjectKind {
                        id: actor_id.object_id(),
                    })?;
                let pool =
                    character
                        .resources
                        .get_mut(resource_id)
                        .ok_or(WorldError::DomainRule {
                            rule: "effect_resource_missing",
                        })?;
                let next = pool.current.checked_add(*amount)?;
                if next < resource.minimum || next > pool.base_maximum || next > resource.maximum {
                    return domain_rule("effect_resource_out_of_range");
                }
                pool.current = next;
                self.world
                    .get_mut::<CharacterComponent>(entity)
                    .ok_or(WorldError::WrongObjectKind {
                        id: actor_id.object_id(),
                    })?
                    .0 = character.clone();
                upserts.push(DomainRecord::Character(character));
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
            EffectDefinition::ApplyCondition {
                condition_id,
                stacks,
                intensity,
            } => self.apply_condition_effect(
                actor_id,
                condition_id,
                *stacks,
                *intensity,
                source_definition_id,
                action_id,
                revision,
                registry,
                ids,
                upserts,
                events,
            )?,
            EffectDefinition::GrantItem { item_id, quantity } => self.apply_grant_item(
                actor_id, item_id, *quantity, action_id, revision, registry, ids, upserts, events,
            )?,
            EffectDefinition::GrantSkill { skill_id, rank } => self.apply_grant_skill(
                actor_id,
                skill_id,
                *rank,
                source_definition_id,
                action_id,
                revision,
                registry,
                ids,
                upserts,
                events,
            )?,
            EffectDefinition::SetParameter {
                parameter_id,
                value,
            } => self.apply_set_parameter(
                actor_id,
                parameter_id,
                value,
                action_id,
                revision,
                registry,
                ids,
                upserts,
                events,
            )?,
            EffectDefinition::EmitEvent { event_type } => events.push(event(
                ids,
                action_id,
                actor_id,
                revision,
                WorldEventKind::DeclarativeEventEmitted {
                    event_type: event_type.clone(),
                    source_definition_id: source_definition_id.clone(),
                },
            )?),
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_condition_effect(
        &mut self,
        actor_id: ActorId,
        condition_id: &ContentDefinitionId,
        stacks: NonZeroU32,
        intensity: Fixed,
        source_definition_id: &ContentDefinitionId,
        action_id: ActionId,
        revision: Revision,
        registry: &DefinitionRegistry,
        ids: &mut impl IdGenerator,
        upserts: &mut Vec<DomainRecord>,
        events: &mut Vec<WorldEvent>,
    ) -> Result<(), WorldError> {
        let entry = registry
            .get(condition_id)
            .ok_or_else(|| WorldError::DefinitionNotFound {
                id: condition_id.clone(),
            })?;
        let Definition::Condition(definition) = &entry.definition else {
            return Err(WorldError::DefinitionNotFound {
                id: condition_id.clone(),
            });
        };
        let definition = definition.clone();
        let origin = entry.origin.clone();
        let mut existing = self
            .objects
            .iter()
            .filter_map(|(id, entity)| {
                self.world
                    .get::<ConditionComponent>(*entity)
                    .filter(|condition| {
                        condition.0.target_id == actor_id
                            && condition.0.condition_id == *condition_id
                    })
                    .map(|condition| (*id, *entity, condition.0.clone()))
            })
            .collect::<Vec<_>>();
        existing.sort_by_key(|(id, _, _)| *id);
        let clock = self.world_state().clock;

        let instance_id = match definition.stack_policy {
            StackPolicy::Unique if !existing.is_empty() => {
                return domain_rule("condition_unique_exists");
            }
            StackPolicy::RefreshDuration if !existing.is_empty() => {
                if existing.len() != 1 {
                    return invariant("condition_refresh_instance_count");
                }
                let (id, entity, mut record) = existing.remove(0);
                record.intensity =
                    merged_intensity(record.intensity, intensity, definition.intensity_policy);
                refresh_condition_timing(&mut record, &definition, clock)?;
                self.world
                    .get_mut::<ConditionComponent>(entity)
                    .ok_or(WorldError::WrongObjectKind { id })?
                    .0 = record.clone();
                upserts.push(DomainRecord::Condition(record));
                id
            }
            StackPolicy::IncreaseStacks {
                maximum,
                refresh_duration,
            } if !existing.is_empty() => {
                if existing.len() != 1 {
                    return invariant("condition_stack_instance_count");
                }
                let (id, entity, mut record) = existing.remove(0);
                let total = record.stacks.get().checked_add(stacks.get()).ok_or(
                    WorldError::DomainRule {
                        rule: "condition_stack_maximum",
                    },
                )?;
                if total > maximum.get() {
                    return domain_rule("condition_stack_maximum");
                }
                record.stacks = NonZeroU32::new(total).ok_or(WorldError::Invariant {
                    invariant: "condition_stack_nonzero",
                })?;
                record.intensity =
                    merged_intensity(record.intensity, intensity, definition.intensity_policy);
                if refresh_duration {
                    refresh_condition_timing(&mut record, &definition, clock)?;
                }
                self.world
                    .get_mut::<ConditionComponent>(entity)
                    .ok_or(WorldError::WrongObjectKind { id })?
                    .0 = record.clone();
                upserts.push(DomainRecord::Condition(record));
                id
            }
            StackPolicy::IndependentInstances { maximum }
                if existing.len() >= maximum.get() as usize =>
            {
                return domain_rule("condition_instance_maximum");
            }
            StackPolicy::IncreaseStacks { maximum, .. } if stacks.get() > maximum.get() => {
                return domain_rule("condition_stack_maximum");
            }
            StackPolicy::Unique
            | StackPolicy::RefreshDuration
            | StackPolicy::IncreaseStacks { .. }
            | StackPolicy::IndependentInstances { .. } => {
                let id = ObjectId::generate_with(ids)?;
                let expires_at = condition_expiry(&definition.duration, clock)?;
                let next_periodic_at = condition_periodic(&definition, clock)?;
                let record = ConditionRecord {
                    id,
                    target_id: actor_id,
                    condition_id: condition_id.clone(),
                    source: ConditionSource::System {
                        source_id: source_definition_id.clone(),
                    },
                    stacks,
                    intensity,
                    applied_at: clock,
                    expires_at,
                    next_periodic_at,
                    origin: EntityOrigin::Content { origin },
                };
                self.spawn_record(DomainRecord::Condition(record.clone()))?;
                upserts.push(DomainRecord::Condition(record));
                id
            }
        };
        events.push(event(
            ids,
            action_id,
            actor_id,
            revision,
            WorldEventKind::ConditionApplied {
                character_id: actor_id,
                condition_id: condition_id.clone(),
                instance_id,
            },
        )?);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_grant_item(
        &mut self,
        actor_id: ActorId,
        item_id: &ContentDefinitionId,
        quantity: NonZeroU32,
        action_id: ActionId,
        revision: Revision,
        registry: &DefinitionRegistry,
        ids: &mut impl IdGenerator,
        upserts: &mut Vec<DomainRecord>,
        events: &mut Vec<WorldEvent>,
    ) -> Result<(), WorldError> {
        let entry = registry
            .get(item_id)
            .ok_or_else(|| WorldError::DefinitionNotFound {
                id: item_id.clone(),
            })?;
        let Definition::Item(definition) = &entry.definition else {
            return Err(WorldError::DefinitionNotFound {
                id: item_id.clone(),
            });
        };
        if quantity.get() > definition.stack_limit.get()
            || (definition.container.is_some() && quantity.get() != 1)
        {
            return domain_rule("grant_item_stack_invalid");
        }
        let root_id = self
            .character(actor_id)
            .ok_or(WorldError::WrongObjectKind {
                id: actor_id.object_id(),
            })?
            .inventory_root;
        let root = self
            .item(root_id)
            .cloned()
            .ok_or(WorldError::WrongObjectKind { id: root_id })?;
        let capacity = root
            .container
            .ok_or(WorldError::WrongObjectKind { id: root_id })?;
        if self
            .items()
            .filter(|item| item.contained_by == Some(root_id))
            .count()
            >= capacity.max_children as usize
        {
            return domain_rule("container_child_capacity");
        }
        let quantity_weight = Fixed::from_integer(i64::from(quantity.get()))?;
        let added_weight = definition.unit_weight_grams.checked_mul(quantity_weight)?;
        if self
            .container_weight(root_id, registry)?
            .checked_add(added_weight)?
            > capacity.max_weight_grams
        {
            return domain_rule("container_weight_capacity");
        }
        let id = ObjectId::generate_with(ids)?;
        let record =
            item_from_definition(id, root_id, actor_id, quantity, definition, &entry.origin);
        self.spawn_record(DomainRecord::Item(record.clone()))?;
        upserts.push(DomainRecord::Item(record));
        events.push(event(
            ids,
            action_id,
            actor_id,
            revision,
            WorldEventKind::ItemGranted {
                character_id: actor_id,
                item_id: id,
                definition_id: item_id.clone(),
                quantity: quantity.get(),
            },
        )?);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_grant_skill(
        &mut self,
        actor_id: ActorId,
        skill_id: &ContentDefinitionId,
        rank: NonZeroU32,
        source_definition_id: &ContentDefinitionId,
        action_id: ActionId,
        revision: Revision,
        registry: &DefinitionRegistry,
        ids: &mut impl IdGenerator,
        upserts: &mut Vec<DomainRecord>,
        events: &mut Vec<WorldEvent>,
    ) -> Result<(), WorldError> {
        let entry = registry
            .get(skill_id)
            .ok_or_else(|| WorldError::DefinitionNotFound {
                id: skill_id.clone(),
            })?;
        if !matches!(entry.definition, Definition::Skill(_)) {
            return Err(WorldError::DefinitionNotFound {
                id: skill_id.clone(),
            });
        }
        if self.objects.values().any(|entity| {
            self.world
                .get::<SkillGrantComponent>(*entity)
                .is_some_and(|grant| grant.0.owner_id == actor_id && grant.0.skill_id == *skill_id)
        }) {
            return domain_rule("skill_already_granted");
        }
        let id = ObjectId::generate_with(ids)?;
        let record = SkillGrantRecord {
            id,
            owner_id: actor_id,
            skill_id: skill_id.clone(),
            rank: rank.get(),
            proficiency: 0,
            source: SkillSource::Rule {
                rule_id: source_definition_id.clone(),
            },
            enabled: true,
            ready_at: None,
            origin: EntityOrigin::Content {
                origin: entry.origin.clone(),
            },
        };
        self.spawn_record(DomainRecord::SkillGrant(record.clone()))?;
        upserts.push(DomainRecord::SkillGrant(record));
        events.push(event(
            ids,
            action_id,
            actor_id,
            revision,
            WorldEventKind::SkillGranted {
                character_id: actor_id,
                grant_id: id,
                skill_id: skill_id.clone(),
            },
        )?);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_set_parameter(
        &mut self,
        actor_id: ActorId,
        parameter_id: &ContentDefinitionId,
        value: &ParameterValue,
        action_id: ActionId,
        revision: Revision,
        registry: &DefinitionRegistry,
        ids: &mut impl IdGenerator,
        upserts: &mut Vec<DomainRecord>,
        events: &mut Vec<WorldEvent>,
    ) -> Result<(), WorldError> {
        let entry = registry
            .get(parameter_id)
            .ok_or_else(|| WorldError::DefinitionNotFound {
                id: parameter_id.clone(),
            })?;
        let Definition::Parameter(definition) = &entry.definition else {
            return Err(WorldError::DefinitionNotFound {
                id: parameter_id.clone(),
            });
        };
        self.validate_parameter_value(&definition.value_type, value)?;
        match definition.persistence {
            ParameterPersistence::Save => {
                let matching = self
                    .objects
                    .iter()
                    .filter_map(|(id, entity)| {
                        self.world
                            .get::<ParameterSetComponent>(*entity)
                            .filter(|set| set.0.schema_id == entry.origin.pack_id)
                            .map(|_| (*id, *entity))
                    })
                    .collect::<Vec<_>>();
                if matching.len() != 1 {
                    return invariant("parameter_set_pack_unique");
                }
                let (id, entity) = matching[0];
                let mut set = self
                    .world
                    .get::<ParameterSetComponent>(entity)
                    .map(|component| component.0.clone())
                    .ok_or(WorldError::WrongObjectKind { id })?;
                set.values.insert(parameter_id.clone(), value.clone());
                self.world
                    .get_mut::<ParameterSetComponent>(entity)
                    .ok_or(WorldError::WrongObjectKind { id })?
                    .0 = set.clone();
                upserts.push(DomainRecord::ParameterSet(set));
            }
            ParameterPersistence::Session => {
                self.session_parameters
                    .insert(parameter_id.clone(), value.clone());
            }
        }
        events.push(event(
            ids,
            action_id,
            actor_id,
            revision,
            WorldEventKind::ParameterChanged {
                parameter_id: parameter_id.clone(),
            },
        )?);
        Ok(())
    }
}

fn merged_intensity(current: Fixed, incoming: Fixed, policy: IntensityPolicy) -> Fixed {
    match policy {
        IntensityPolicy::Keep => current,
        IntensityPolicy::Replace => incoming,
        IntensityPolicy::Maximum => current.max(incoming),
    }
}

fn condition_expiry(
    duration: &DurationPolicy,
    clock: WorldTime,
) -> Result<Option<WorldTime>, WorldError> {
    match duration {
        DurationPolicy::Permanent => Ok(None),
        DurationPolicy::Finite { ticks } => Ok(Some(clock.checked_add(ticks.get())?)),
    }
}

fn condition_periodic(
    definition: &loreloom_content::ConditionDefinition,
    clock: WorldTime,
) -> Result<Option<WorldTime>, WorldError> {
    definition
        .periodic
        .as_ref()
        .map(|periodic| clock.checked_add(periodic.interval_ticks.get()))
        .transpose()
        .map_err(WorldError::from)
}

fn refresh_condition_timing(
    record: &mut ConditionRecord,
    definition: &loreloom_content::ConditionDefinition,
    clock: WorldTime,
) -> Result<(), WorldError> {
    record.expires_at = condition_expiry(&definition.duration, clock)?;
    record.next_periodic_at = condition_periodic(definition, clock)?;
    Ok(())
}

impl GameWorld {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn run_declarative_rules(
        &mut self,
        actor_id: ActorId,
        action_id: ActionId,
        revision: Revision,
        registry: &DefinitionRegistry,
        ids: &mut impl IdGenerator,
        upserts: &mut Vec<DomainRecord>,
        events: &mut Vec<WorldEvent>,
        budget: &mut ExecutionBudget,
    ) -> Result<(), WorldError> {
        let mut signals = VecDeque::new();
        for event in events.clone() {
            self.enqueue_event_signals(&event, 0, &mut signals)?;
        }
        while let Some(signal) = signals.pop_front() {
            let mut candidates = Vec::new();
            for (_, entry) in registry.iter() {
                let Definition::Rule(rule) = &entry.definition else {
                    continue;
                };
                let occurrences = trigger_occurrences(&rule.trigger, &signal);
                if occurrences == 0 {
                    continue;
                }
                let state = self.rule_state_for_definition(&rule.id)?;
                candidates.push((
                    rule.priority,
                    rule.id.clone(),
                    state.id,
                    rule.clone(),
                    occurrences,
                ));
            }
            if candidates.is_empty() {
                continue;
            }
            if signal.depth() > self.config.rule_limits.max_cascade_depth {
                return domain_rule("rule_cascade_depth");
            }
            candidates.sort_by(|left, right| {
                (&left.0, &left.1, &left.2).cmp(&(&right.0, &right.1, &right.2))
            });
            for (_, _, state_id, rule, occurrences) in candidates {
                for _ in 0..occurrences {
                    if !self.evaluate_predicates(actor_id, &rule.predicates, budget)? {
                        continue;
                    }
                    budget.charge_rule(self.config.rule_limits)?;
                    let event_start = events.len();
                    self.apply_effects(
                        actor_id,
                        &rule.effects,
                        &rule.id,
                        action_id,
                        revision,
                        registry,
                        ids,
                        budget,
                        upserts,
                        events,
                    )?;
                    let state_entity = self.require_object(state_id)?;
                    let mut state = self
                        .world
                        .get::<RuleStateComponent>(state_entity)
                        .map(|component| component.0.clone())
                        .ok_or(WorldError::WrongObjectKind { id: state_id })?;
                    state.trigger_count =
                        state
                            .trigger_count
                            .checked_add(1)
                            .ok_or(WorldError::DomainRule {
                                rule: "rule_trigger_count_overflow",
                            })?;
                    state.last_triggered_at = Some(self.world_state().clock);
                    self.world
                        .get_mut::<RuleStateComponent>(state_entity)
                        .ok_or(WorldError::WrongObjectKind { id: state_id })?
                        .0 = state.clone();
                    upserts.push(DomainRecord::RuleState(state));
                    events.push(event(
                        ids,
                        action_id,
                        actor_id,
                        revision,
                        WorldEventKind::RuleTriggered {
                            rule_id: rule.id.clone(),
                            trigger: signal.label()?,
                        },
                    )?);
                    let next_depth =
                        signal
                            .depth()
                            .checked_add(1)
                            .ok_or(WorldError::DomainRule {
                                rule: "rule_cascade_depth",
                            })?;
                    for generated in &events[event_start..] {
                        self.enqueue_event_signals(generated, next_depth, &mut signals)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn rule_state_for_definition(
        &self,
        definition_id: &ContentDefinitionId,
    ) -> Result<RuleStateRecord, WorldError> {
        let states = self
            .objects
            .values()
            .filter_map(|entity| self.world.get::<RuleStateComponent>(*entity))
            .filter(|state| state.0.definition_id == *definition_id)
            .map(|state| state.0.clone())
            .collect::<Vec<_>>();
        if states.len() != 1 {
            return invariant("rule_state_definition_unique");
        }
        Ok(states[0].clone())
    }

    fn enqueue_event_signals(
        &self,
        event: &WorldEvent,
        depth: u32,
        signals: &mut VecDeque<RuleSignal>,
    ) -> Result<(), WorldError> {
        let Some(event_type) = world_event_type(&event.kind)? else {
            return Ok(());
        };
        signals.push_back(RuleSignal::WorldEvent { event_type, depth });
        match &event.kind {
            WorldEventKind::GameplayActionPerformed { action_id } => {
                signals.push_back(RuleSignal::GameplayAction {
                    action_id: action_id.clone(),
                    depth,
                });
            }
            WorldEventKind::ClockAdvanced { from, to } => {
                signals.push_back(RuleSignal::WorldClock {
                    from: *from,
                    to: *to,
                    depth,
                });
            }
            WorldEventKind::SceneLeft { scene_id } => {
                if let Some(origin) = self.scene(*scene_id)?.origin.content() {
                    signals.push_back(RuleSignal::SceneLeft {
                        scene_definition_id: origin.definition_id.clone(),
                        depth,
                    });
                }
            }
            WorldEventKind::SceneEntered { scene_id } => {
                if let Some(origin) = self.scene(*scene_id)?.origin.content() {
                    signals.push_back(RuleSignal::SceneEntered {
                        scene_definition_id: origin.definition_id.clone(),
                        depth,
                    });
                }
            }
            _ => {}
        }
        Ok(())
    }
}

fn trigger_occurrences(trigger: &TriggerDefinition, signal: &RuleSignal) -> u64 {
    match (trigger, signal) {
        (
            TriggerDefinition::WorldEvent {
                event_type: expected,
            },
            RuleSignal::WorldEvent { event_type, .. },
        ) if expected == event_type => 1,
        (
            TriggerDefinition::GameplayAction {
                action_id: expected,
            },
            RuleSignal::GameplayAction { action_id, .. },
        ) if expected == action_id => 1,
        (
            TriggerDefinition::SceneEntered { scene_id },
            RuleSignal::SceneEntered {
                scene_definition_id,
                ..
            },
        ) if scene_id == scene_definition_id => 1,
        (
            TriggerDefinition::SceneLeft { scene_id },
            RuleSignal::SceneLeft {
                scene_definition_id,
                ..
            },
        ) if scene_id == scene_definition_id => 1,
        (
            TriggerDefinition::WorldClock { every_ticks },
            RuleSignal::WorldClock { from, to, .. },
        ) => to / every_ticks.get() - from / every_ticks.get(),
        _ => 0,
    }
}

fn world_event_type(kind: &WorldEventKind) -> Result<Option<ShortText>, WorldError> {
    let value = match kind {
        WorldEventKind::CharacterMoved { .. } => "character_moved",
        WorldEventKind::ItemTransferred { .. } => "item_transferred",
        WorldEventKind::ItemEquipped { .. } => "item_equipped",
        WorldEventKind::StackSplit { .. } => "stack_split",
        WorldEventKind::SkillUsed { .. } => "skill_used",
        WorldEventKind::ClockAdvanced { .. } => "clock_advanced",
        WorldEventKind::CharacterSpawned { .. } => "character_spawned",
        WorldEventKind::CharacterPromoted { .. } => "character_promoted",
        WorldEventKind::SceneLeft { .. } => "scene_left",
        WorldEventKind::SceneEntered { .. } => "scene_entered",
        WorldEventKind::SceneCreated { .. } => "scene_created",
        WorldEventKind::PlaceCreated { .. } => "place_created",
        WorldEventKind::ConditionExpired { .. } => "condition_expired",
        WorldEventKind::ConditionTicked { .. } => "condition_ticked",
        WorldEventKind::ResourceChanged { .. } => "resource_changed",
        WorldEventKind::ConditionApplied { .. } => "condition_applied",
        WorldEventKind::ItemGranted { .. } => "item_granted",
        WorldEventKind::SkillGranted { .. } => "skill_granted",
        WorldEventKind::ParameterChanged { .. } => "parameter_changed",
        WorldEventKind::GameplayActionPerformed { .. } => "gameplay_action_performed",
        WorldEventKind::EventOptionChosen { .. } => "event_option_chosen",
        WorldEventKind::DeclarativeEventEmitted { event_type, .. } => {
            return Ok(Some(event_type.clone()));
        }
        WorldEventKind::RuleTriggered { .. } => return Ok(None),
    };
    ShortText::new(value)
        .map(Some)
        .map_err(|_| WorldError::Invariant {
            invariant: "world_event_type_bound",
        })
}
