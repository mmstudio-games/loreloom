use std::collections::{BTreeMap, BTreeSet};

use loreloom_core::{
    AgentBinding, CharacterController, CharacterLifetime, CharacterSpawnSpec, ConditionGrantInput,
    ConditionSource, ContentDefinitionId, ContentHash, ContentOrigin, DisplayName,
    DomainValueError, EntityOrigin, FactSource, Fixed, GeneratedOrigin, GoalInput, GoalSource,
    ItemGrantInput, KnowledgeStatus, KnownFactInput, ModId, PlacementInput, ResourcePool,
    ShortText, SkillGrantInput,
};
use semver::Version;
use thiserror::Error;

use crate::schema::{
    CharacterDefinition, ContentDocument, Definition, EffectDefinition, EventDefinition,
    GenerationPolicy, InitialCharacterController, InitialCharacterLifetime, NpcDraft,
    ParameterDefinition, ParameterType, PredicateDefinition, SkillKind, SkillTarget,
    TriggerDefinition,
};

pub const CONTENT_SCHEMA_V1: u32 = 1;

#[derive(Debug, Error)]
pub enum ContentError {
    #[error("unsupported content schema {observed}")]
    UnsupportedSchema { observed: u32 },
    #[error("definition {id} has kind {observed}, expected {expected}")]
    DefinitionKind {
        id: ContentDefinitionId,
        observed: String,
        expected: &'static str,
    },
    #[error("definition {id} does not belong to mod {mod_id}")]
    WrongMod {
        id: ContentDefinitionId,
        mod_id: ModId,
    },
    #[error("duplicate definition {id}")]
    DuplicateDefinition { id: ContentDefinitionId },
    #[error("duplicate content pack {id}")]
    DuplicatePack { id: ContentDefinitionId },
    #[error("definition {owner} references missing or wrong-kind {target}; expected {expected}")]
    InvalidReference {
        owner: ContentDefinitionId,
        target: ContentDefinitionId,
        expected: &'static str,
    },
    #[error("invalid content value in {id}: {field}")]
    InvalidValue {
        id: ContentDefinitionId,
        field: &'static str,
    },
    #[error("character definition {id} cannot be compiled: {reason}")]
    CompileCharacter {
        id: ContentDefinitionId,
        reason: &'static str,
    },
    #[error("scene definition {id} cannot be compiled: {reason}")]
    CompileScene {
        id: ContentDefinitionId,
        reason: &'static str,
    },
    #[error("content text field exceeds its bound")]
    TextBound,
    #[error(transparent)]
    DomainValue(#[from] DomainValueError),
    #[error("invalid definition identifier")]
    Identity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentPackContext {
    pub mod_id: ModId,
    pub mod_version: Version,
    pub pack_id: ContentDefinitionId,
    pub content_version: u32,
    pub content_hash: ContentHash,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredDefinition {
    pub definition: Definition,
    pub origin: ContentOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionRegistry {
    contexts: BTreeMap<ContentDefinitionId, ContentPackContext>,
    definitions: BTreeMap<ContentDefinitionId, RegisteredDefinition>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterCompileRequest {
    pub scene_id: loreloom_core::ObjectId,
    pub place_id: loreloom_core::ObjectId,
    pub controller: CharacterController,
    pub lifetime: CharacterLifetime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftCompileRequest {
    pub origin: GeneratedOrigin,
    pub scene_id: loreloom_core::ObjectId,
    pub place_id: loreloom_core::ObjectId,
    pub controller: CharacterController,
    pub lifetime: CharacterLifetime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenePlaceSpawnPlan {
    pub definition_id: ContentDefinitionId,
    pub display_name: DisplayName,
    pub description: ShortText,
    pub tags: BTreeSet<ContentDefinitionId>,
    pub origin: ContentOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneCharacterSpawnPlan {
    pub local_key: ShortText,
    pub character_id: ContentDefinitionId,
    pub place_id: ContentDefinitionId,
    pub controller: CharacterController,
    pub lifetime: InitialCharacterLifetime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneSpawnPlan {
    pub definition_id: ContentDefinitionId,
    pub display_name: DisplayName,
    pub framing: ShortText,
    pub entry_place: ContentDefinitionId,
    pub places: Vec<ScenePlaceSpawnPlan>,
    pub characters: Vec<SceneCharacterSpawnPlan>,
    pub origin: ContentOrigin,
}

impl DefinitionRegistry {
    pub fn build(
        context: ContentPackContext,
        documents: impl IntoIterator<Item = ContentDocument>,
    ) -> Result<Self, ContentError> {
        Self::build_packages([(context, documents.into_iter().collect())])
    }

    pub fn build_packages(
        packages: impl IntoIterator<Item = (ContentPackContext, Vec<ContentDocument>)>,
    ) -> Result<Self, ContentError> {
        let mut contexts = BTreeMap::new();
        let mut definitions = BTreeMap::new();
        for (context, documents) in packages {
            if context
                .pack_id
                .mod_id()
                .map_err(|_| ContentError::Identity)?
                != context.mod_id
                || context.pack_id.kind().map_err(|_| ContentError::Identity)? != "pack"
            {
                return Err(ContentError::WrongMod {
                    id: context.pack_id.clone(),
                    mod_id: context.mod_id.clone(),
                });
            }
            if contexts
                .insert(context.pack_id.clone(), context.clone())
                .is_some()
            {
                return Err(ContentError::DuplicatePack {
                    id: context.pack_id,
                });
            }
            let version_text = ShortText::new(context.mod_version.to_string())
                .map_err(|_| ContentError::TextBound)?;
            for document in documents {
                if document.schema_version != CONTENT_SCHEMA_V1 {
                    return Err(ContentError::UnsupportedSchema {
                        observed: document.schema_version,
                    });
                }
                for definition in document.definitions {
                    validate_definition_identity(&context.mod_id, &definition)?;
                    validate_local_values(&definition)?;
                    let id = definition.id().clone();
                    let origin = ContentOrigin {
                        mod_id: context.mod_id.clone(),
                        mod_version: version_text.clone(),
                        pack_id: context.pack_id.clone(),
                        definition_id: id.clone(),
                        content_version: context.content_version,
                        content_hash: context.content_hash.clone(),
                    };
                    if definitions
                        .insert(id.clone(), RegisteredDefinition { definition, origin })
                        .is_some()
                    {
                        return Err(ContentError::DuplicateDefinition { id });
                    }
                }
            }
        }
        validate_references(&definitions)?;
        Ok(Self {
            contexts,
            definitions,
        })
    }

    pub fn contexts(&self) -> impl Iterator<Item = &ContentPackContext> {
        self.contexts.values()
    }

    #[must_use]
    pub fn get(&self, id: &ContentDefinitionId) -> Option<&RegisteredDefinition> {
        self.definitions.get(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&ContentDefinitionId, &RegisteredDefinition)> {
        self.definitions.iter()
    }

    pub fn compile_scene(&self, id: &ContentDefinitionId) -> Result<SceneSpawnPlan, ContentError> {
        let entry = self
            .definitions
            .get(id)
            .ok_or_else(|| ContentError::CompileScene {
                id: id.clone(),
                reason: "definition is missing",
            })?;
        let Definition::Scene(scene) = &entry.definition else {
            return Err(ContentError::CompileScene {
                id: id.clone(),
                reason: "definition is not a scene",
            });
        };
        let mut places = Vec::with_capacity(scene.places.len());
        for place_id in &scene.places {
            let Some(place_entry) = self.definitions.get(place_id) else {
                return Err(ContentError::CompileScene {
                    id: id.clone(),
                    reason: "place is unavailable",
                });
            };
            let Definition::Place(place) = &place_entry.definition else {
                return Err(ContentError::CompileScene {
                    id: id.clone(),
                    reason: "place has the wrong kind",
                });
            };
            places.push(ScenePlaceSpawnPlan {
                definition_id: place.id.clone(),
                display_name: place.display_name.clone(),
                description: place.description.clone(),
                tags: place.tags.clone(),
                origin: place_entry.origin.clone(),
            });
        }
        let mut characters = scene.characters.clone();
        characters.sort_by(|left, right| left.local_key.cmp(&right.local_key));
        if characters
            .iter()
            .filter(|character| character.controller == InitialCharacterController::Player)
            .count()
            != 1
        {
            return Err(ContentError::CompileScene {
                id: id.clone(),
                reason: "bootstrap scene requires exactly one player",
            });
        }
        Ok(SceneSpawnPlan {
            definition_id: scene.id.clone(),
            display_name: scene.display_name.clone(),
            framing: scene.framing.clone(),
            entry_place: scene.entry_place.clone(),
            places,
            characters: characters
                .into_iter()
                .map(|character| SceneCharacterSpawnPlan {
                    local_key: character.local_key,
                    character_id: character.character_id,
                    place_id: character.place_id,
                    controller: match character.controller {
                        InitialCharacterController::Player => CharacterController::Player,
                        InitialCharacterController::Narrator => CharacterController::NarratorProxy,
                        InitialCharacterController::Rules => CharacterController::Rules,
                        InitialCharacterController::Agent => CharacterController::Agent,
                    },
                    lifetime: character.lifetime,
                })
                .collect(),
            origin: entry.origin.clone(),
        })
    }

    pub fn compile_character(
        &self,
        id: &ContentDefinitionId,
        request: CharacterCompileRequest,
    ) -> Result<CharacterSpawnSpec, ContentError> {
        let entry = self
            .definitions
            .get(id)
            .ok_or_else(|| ContentError::CompileCharacter {
                id: id.clone(),
                reason: "definition is missing",
            })?;
        let Definition::Character(character) = &entry.definition else {
            return Err(ContentError::CompileCharacter {
                id: id.clone(),
                reason: "definition is not a character",
            });
        };
        let agent_binding = match (request.controller, &character.agent_profile) {
            (CharacterController::Agent, Some(profile_id)) => Some(AgentBinding {
                profile_id: profile_id.clone(),
                enabled: true,
                autonomy: match self
                    .definitions
                    .get(profile_id)
                    .map(|entry| &entry.definition)
                {
                    Some(Definition::AgentProfile(profile)) => profile.autonomy,
                    _ => {
                        return Err(ContentError::CompileCharacter {
                            id: id.clone(),
                            reason: "agent profile is unavailable",
                        });
                    }
                },
            }),
            (CharacterController::Agent, None) => {
                return Err(ContentError::CompileCharacter {
                    id: id.clone(),
                    reason: "Agent controller requires an AgentProfile",
                });
            }
            (_, _) => None,
        };
        let resources = character
            .resources
            .iter()
            .map(|resource| {
                (
                    resource.resource_id.clone(),
                    ResourcePool {
                        resource_id: resource.resource_id.clone(),
                        current: resource.current,
                        base_maximum: resource.base_maximum,
                    },
                )
            })
            .collect();
        let source = ConditionSource::System {
            source_id: character.id.clone(),
        };
        Ok(CharacterSpawnSpec {
            origin: EntityOrigin::Content {
                origin: entry.origin.clone(),
            },
            display_name: character.display_name.clone(),
            profile: character.profile.clone(),
            controller: request.controller,
            lifetime: request.lifetime,
            agent_binding,
            placement: PlacementInput {
                scene_id: request.scene_id,
                place_id: request.place_id,
            },
            attributes: character.base_attributes.clone(),
            resources,
            conditions: character
                .conditions
                .iter()
                .map(|condition| ConditionGrantInput {
                    condition_id: condition.condition_id.clone(),
                    source: source.clone(),
                    stacks: condition.stacks,
                    intensity: condition.intensity,
                })
                .collect(),
            inventory: character
                .inventory
                .iter()
                .map(|item| ItemGrantInput {
                    local_key: item.local_key.clone(),
                    definition_id: item.item_id.clone(),
                    quantity: item.quantity,
                    parent_local_key: item.parent_local_key.clone(),
                })
                .collect(),
            skills: character
                .skills
                .iter()
                .map(|skill| SkillGrantInput {
                    skill_id: skill.skill_id.clone(),
                    rank: skill.rank.get(),
                    proficiency: skill.proficiency,
                    enabled: skill.enabled,
                })
                .collect(),
            knowledge: character
                .knowledge
                .iter()
                .map(|fact| KnownFactInput {
                    subject: fact.subject,
                    predicate_id: fact.predicate_id.clone(),
                    value: fact.value.clone(),
                    status: KnowledgeStatus::Believed,
                    confidence: fact.confidence,
                    source: FactSource::Content {
                        definition_id: character.id.clone(),
                    },
                })
                .collect(),
            goals: character
                .goals
                .iter()
                .map(|goal| GoalInput {
                    description: goal.description.clone(),
                    priority: goal.priority,
                    source: GoalSource::CharacterDefinition {
                        definition_id: character.id.clone(),
                    },
                })
                .collect(),
            trusted_constraints: character.spawn_constraints.clone(),
        })
    }

    pub fn compile_draft(
        &self,
        draft: &NpcDraft,
        policy: &GenerationPolicy,
        request: DraftCompileRequest,
    ) -> Result<CharacterSpawnSpec, ContentError> {
        validate_draft_references(&self.definitions, policy, draft)?;
        validate_draft_budget(policy, draft)?;
        let agent_binding = match (request.controller, &draft.agent_profile) {
            (CharacterController::Agent, Some(profile_id))
                if policy.allowed_agent_profiles.contains(profile_id) =>
            {
                let Some(Definition::AgentProfile(profile)) = self
                    .definitions
                    .get(profile_id)
                    .map(|entry| &entry.definition)
                else {
                    return Err(ContentError::CompileCharacter {
                        id: policy.id.clone(),
                        reason: "draft AgentProfile is unavailable",
                    });
                };
                Some(AgentBinding {
                    profile_id: profile_id.clone(),
                    enabled: true,
                    autonomy: profile.autonomy,
                })
            }
            (CharacterController::Agent, _) => {
                return Err(ContentError::CompileCharacter {
                    id: policy.id.clone(),
                    reason: "draft Agent controller is not allowed by GenerationPolicy",
                });
            }
            (_, _) => None,
        };
        let resources = draft
            .resources
            .iter()
            .map(|resource| {
                (
                    resource.resource_id.clone(),
                    ResourcePool {
                        resource_id: resource.resource_id.clone(),
                        current: resource.current,
                        base_maximum: resource.base_maximum,
                    },
                )
            })
            .collect();
        let source = ConditionSource::System {
            source_id: policy.id.clone(),
        };
        Ok(CharacterSpawnSpec {
            origin: EntityOrigin::Generated {
                origin: request.origin,
            },
            display_name: draft.display_name.clone(),
            profile: draft.profile.clone(),
            controller: request.controller,
            lifetime: request.lifetime,
            agent_binding,
            placement: PlacementInput {
                scene_id: request.scene_id,
                place_id: request.place_id,
            },
            attributes: draft.base_attributes.clone(),
            resources,
            conditions: draft
                .conditions
                .iter()
                .map(|condition| ConditionGrantInput {
                    condition_id: condition.condition_id.clone(),
                    source: source.clone(),
                    stacks: condition.stacks,
                    intensity: condition.intensity,
                })
                .collect(),
            inventory: draft
                .inventory
                .iter()
                .map(|item| ItemGrantInput {
                    local_key: item.local_key.clone(),
                    definition_id: item.item_id.clone(),
                    quantity: item.quantity,
                    parent_local_key: item.parent_local_key.clone(),
                })
                .collect(),
            skills: draft
                .skills
                .iter()
                .map(|skill| SkillGrantInput {
                    skill_id: skill.skill_id.clone(),
                    rank: skill.rank.get(),
                    proficiency: skill.proficiency,
                    enabled: skill.enabled,
                })
                .collect(),
            knowledge: draft
                .knowledge
                .iter()
                .map(|fact| KnownFactInput {
                    subject: fact.subject,
                    predicate_id: fact.predicate_id.clone(),
                    value: fact.value.clone(),
                    status: KnowledgeStatus::Believed,
                    confidence: fact.confidence,
                    source: FactSource::Content {
                        definition_id: policy.id.clone(),
                    },
                })
                .collect(),
            goals: draft
                .goals
                .iter()
                .map(|goal| GoalInput {
                    description: goal.description.clone(),
                    priority: goal.priority,
                    source: GoalSource::Rule {
                        rule_id: policy.id.clone(),
                    },
                })
                .collect(),
            trusted_constraints: policy.constraints.clone(),
        })
    }
}

fn validate_draft_references(
    definitions: &BTreeMap<ContentDefinitionId, RegisteredDefinition>,
    policy: &GenerationPolicy,
    draft: &NpcDraft,
) -> Result<(), ContentError> {
    if let Some(agent_profile) = &draft.agent_profile {
        require_kind(definitions, &policy.id, agent_profile, "agent_profile")?;
    }
    for agent_profile in &policy.allowed_agent_profiles {
        require_kind(definitions, &policy.id, agent_profile, "agent_profile")?;
    }
    for tag in &draft.profile.narrative_tags {
        require_kind(definitions, &policy.id, tag, "tag")?;
    }
    for attribute in draft.base_attributes.0.keys() {
        require_kind(definitions, &policy.id, attribute, "attribute")?;
    }
    for attribute in policy
        .constraints
        .minimum_attributes
        .keys()
        .chain(policy.constraints.maximum_attributes.keys())
    {
        require_kind(definitions, &policy.id, attribute, "attribute")?;
    }
    for definition_id in &policy.constraints.allowed_definitions {
        require_any_kind(
            definitions,
            &policy.id,
            definition_id,
            &["condition", "item", "skill"],
            "condition, item, or skill",
        )?;
    }
    for resource in &draft.resources {
        require_kind(definitions, &policy.id, &resource.resource_id, "resource")?;
    }
    for condition in &draft.conditions {
        require_kind(
            definitions,
            &policy.id,
            &condition.condition_id,
            "condition",
        )?;
    }
    for item in &draft.inventory {
        require_kind(definitions, &policy.id, &item.item_id, "item")?;
    }
    for skill in &draft.skills {
        require_kind(definitions, &policy.id, &skill.skill_id, "skill")?;
    }
    for fact in &draft.knowledge {
        require_kind(definitions, &policy.id, &fact.predicate_id, "predicate")?;
    }
    Ok(())
}

fn validate_draft_budget(policy: &GenerationPolicy, draft: &NpcDraft) -> Result<(), ContentError> {
    let invalid = |field| ContentError::InvalidValue {
        id: policy.id.clone(),
        field,
    };
    if draft.inventory.len() > policy.constraints.maximum_items as usize {
        return Err(invalid("draft.inventory.budget"));
    }
    if draft.skills.len() > policy.constraints.maximum_skills as usize {
        return Err(invalid("draft.skills.budget"));
    }
    if policy.constraints.maximum_attribute_points < Fixed::ZERO
        || policy.constraints.minimum_attributes.keys().any(|id| {
            policy
                .constraints
                .maximum_attributes
                .get(id)
                .is_none_or(|maximum| policy.constraints.minimum_attributes[id] > *maximum)
        })
        || policy
            .constraints
            .maximum_attributes
            .keys()
            .any(|id| !policy.constraints.minimum_attributes.contains_key(id))
    {
        return Err(invalid("draft.attribute.schema"));
    }
    let used_definitions = draft
        .conditions
        .iter()
        .map(|value| &value.condition_id)
        .chain(draft.inventory.iter().map(|value| &value.item_id))
        .chain(draft.skills.iter().map(|value| &value.skill_id));
    if used_definitions
        .clone()
        .any(|id| !policy.constraints.allowed_definitions.contains(id))
    {
        return Err(invalid("draft.allowed_definitions"));
    }
    let mut points = Fixed::ZERO;
    for (attribute_id, value) in &draft.base_attributes.0 {
        let minimum = policy
            .constraints
            .minimum_attributes
            .get(attribute_id)
            .copied()
            .ok_or_else(|| invalid("draft.attribute.minimum"))?;
        let maximum = policy
            .constraints
            .maximum_attributes
            .get(attribute_id)
            .copied()
            .ok_or_else(|| invalid("draft.attribute.maximum"))?;
        if *value < minimum || *value > maximum {
            return Err(invalid("draft.attribute.range"));
        }
        let spent = value
            .checked_sub(minimum)
            .map_err(|_| invalid("draft.attribute.points"))?;
        points = points
            .checked_add(spent)
            .map_err(|_| invalid("draft.attribute.points"))?;
    }
    if points > policy.constraints.maximum_attribute_points {
        return Err(invalid("draft.attribute.points"));
    }
    Ok(())
}

fn validate_definition_identity(
    mod_id: &ModId,
    definition: &Definition,
) -> Result<(), ContentError> {
    let id = definition.id();
    if id.mod_id().map_err(|_| ContentError::Identity)? != *mod_id {
        return Err(ContentError::WrongMod {
            id: id.clone(),
            mod_id: mod_id.clone(),
        });
    }
    let observed = id.kind().map_err(|_| ContentError::Identity)?;
    if observed != definition.expected_kind() {
        return Err(ContentError::DefinitionKind {
            id: id.clone(),
            observed: observed.to_owned(),
            expected: definition.expected_kind(),
        });
    }
    Ok(())
}

fn validate_local_values(definition: &Definition) -> Result<(), ContentError> {
    let invalid = |field| ContentError::InvalidValue {
        id: definition.id().clone(),
        field,
    };
    match definition {
        Definition::Attribute(value) if value.minimum > value.maximum => {
            Err(invalid("attribute.range"))
        }
        Definition::Resource(value)
            if value.minimum < loreloom_core::Fixed::ZERO || value.maximum <= value.minimum =>
        {
            Err(invalid("resource.range"))
        }
        Definition::Item(value)
            if value.unit_weight_grams < loreloom_core::Fixed::ZERO
                || value.container.is_some_and(|container| {
                    container.max_weight_grams < loreloom_core::Fixed::ZERO
                        || container.max_children == 0
                })
                || value
                    .durability
                    .is_some_and(|durability| durability.maximum <= loreloom_core::Fixed::ZERO) =>
        {
            Err(invalid("item.range"))
        }
        Definition::Skill(value) => validate_skill(value).map_err(invalid),
        Definition::Character(value) => validate_character(value).map_err(invalid),
        Definition::RelationshipKind(value) if value.minimum > value.maximum => {
            Err(invalid("relationship_kind.range"))
        }
        Definition::Parameter(value) => validate_parameter(value).map_err(invalid),
        Definition::Event(value) => validate_event(value).map_err(invalid),
        Definition::GameplayAction(value)
            if value.parameters.len() > 64
                || value.predicates.len() > 64
                || value.effects.len() > 32 =>
        {
            Err(invalid("gameplay_action.budget"))
        }
        Definition::Rule(value) if value.predicates.len() > 64 || value.effects.len() > 32 => {
            Err(invalid("rule.budget"))
        }
        Definition::Scene(value) => {
            if !value.places.contains(&value.entry_place) {
                return Err(invalid("scene.entry_place"));
            }
            let local_keys = value
                .characters
                .iter()
                .map(|character| &character.local_key)
                .collect::<BTreeSet<_>>();
            if local_keys.len() != value.characters.len() {
                return Err(invalid("scene.character.local_key"));
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn validate_parameter(value: &ParameterDefinition) -> Result<(), &'static str> {
    let matches = match (&value.value_type, &value.default) {
        (ParameterType::Bool, loreloom_core::ParameterValue::Bool(_)) => true,
        (
            ParameterType::Fixed { minimum, maximum },
            loreloom_core::ParameterValue::Fixed(value),
        ) => minimum <= maximum && value >= minimum && value <= maximum,
        (
            ParameterType::Counter { minimum, maximum },
            loreloom_core::ParameterValue::Counter(value),
        ) => minimum <= maximum && value >= minimum && value <= maximum,
        (ParameterType::Enum { variants }, loreloom_core::ParameterValue::Enum(value)) => {
            variants.contains(value)
        }
        (
            ParameterType::TagSet { allowed, maximum },
            loreloom_core::ParameterValue::TagSet(values),
        ) => {
            values.len() <= *maximum as usize && values.iter().all(|value| allowed.contains(value))
        }
        (ParameterType::ObjectRef { .. }, loreloom_core::ParameterValue::ObjectRef(_)) => true,
        _ => false,
    };
    if matches {
        Ok(())
    } else {
        Err("parameter.default")
    }
}

fn validate_event(value: &EventDefinition) -> Result<(), &'static str> {
    let nodes = value
        .nodes
        .iter()
        .map(|node| node.id.clone())
        .collect::<BTreeSet<_>>();
    if nodes.len() != value.nodes.len() || !nodes.contains(&value.entry_node) {
        return Err("event.nodes");
    }
    let mut options = BTreeSet::new();
    for node in &value.nodes {
        if node.id.kind().ok() != Some("event_node")
            || node.options.iter().any(|option| {
                option.id.kind().ok() != Some("event_option")
                    || !options.insert(option.id.clone())
                    || option
                        .next_node
                        .as_ref()
                        .is_some_and(|next| !nodes.contains(next))
            })
        {
            return Err("event.options");
        }
    }
    Ok(())
}

fn validate_skill(value: &crate::schema::SkillDefinition) -> Result<(), &'static str> {
    let mut costs = BTreeSet::new();
    if value.costs.iter().any(|cost| {
        cost.amount <= loreloom_core::Fixed::ZERO || !costs.insert(cost.resource_id.clone())
    }) {
        return Err("skill.costs");
    }
    let range = match &value.target {
        SkillTarget::SelfTarget => None,
        SkillTarget::Character { maximum_range, .. }
        | SkillTarget::Object { maximum_range, .. }
        | SkillTarget::Place { maximum_range } => Some(*maximum_range),
    };
    if range.is_some_and(|range| range < loreloom_core::Fixed::ZERO) {
        return Err("skill.target.maximum_range");
    }
    if (value.kind == SkillKind::Reaction) != value.reaction.is_some() {
        return Err("skill.reaction");
    }
    Ok(())
}

fn validate_character(value: &CharacterDefinition) -> Result<(), &'static str> {
    let mut resource_ids = BTreeSet::new();
    if value.resources.iter().any(|resource| {
        resource.current < loreloom_core::Fixed::ZERO
            || resource.base_maximum <= loreloom_core::Fixed::ZERO
            || resource.current > resource.base_maximum
            || !resource_ids.insert(resource.resource_id.clone())
    }) {
        return Err("character.resources");
    }
    let mut local_keys = BTreeSet::new();
    for item in &value.inventory {
        if !local_keys.insert(item.local_key.clone()) {
            return Err("character.inventory.local_key");
        }
    }
    if value.inventory.iter().any(|item| {
        item.parent_local_key
            .as_ref()
            .is_some_and(|parent| !local_keys.contains(parent))
    }) {
        return Err("character.inventory.parent_local_key");
    }
    let constraints = &value.spawn_constraints;
    if value.inventory.len() > constraints.maximum_items as usize
        || value.skills.len() > constraints.maximum_skills as usize
        || constraints.maximum_attribute_points < Fixed::ZERO
        || value
            .conditions
            .iter()
            .map(|condition| &condition.condition_id)
            .chain(value.inventory.iter().map(|item| &item.item_id))
            .chain(value.skills.iter().map(|skill| &skill.skill_id))
            .any(|id| !constraints.allowed_definitions.contains(id))
    {
        return Err("character.spawn_constraints");
    }
    if constraints.minimum_attributes.keys().any(|id| {
        constraints
            .maximum_attributes
            .get(id)
            .is_none_or(|maximum| constraints.minimum_attributes[id] > *maximum)
    }) || constraints
        .maximum_attributes
        .keys()
        .any(|id| !constraints.minimum_attributes.contains_key(id))
    {
        return Err("character.spawn_constraints.attributes");
    }
    let mut points = Fixed::ZERO;
    for (attribute_id, value) in &value.base_attributes.0 {
        let Some(minimum) = constraints.minimum_attributes.get(attribute_id) else {
            return Err("character.spawn_constraints.attribute_minimum");
        };
        let Some(maximum) = constraints.maximum_attributes.get(attribute_id) else {
            return Err("character.spawn_constraints.attribute_maximum");
        };
        if value < minimum || value > maximum {
            return Err("character.spawn_constraints.attribute_range");
        }
        let Ok(spent) = value.checked_sub(*minimum) else {
            return Err("character.spawn_constraints.attribute_points");
        };
        let Ok(total) = points.checked_add(spent) else {
            return Err("character.spawn_constraints.attribute_points");
        };
        points = total;
    }
    if points > constraints.maximum_attribute_points {
        return Err("character.spawn_constraints.attribute_points");
    }
    if value
        .goals
        .iter()
        .any(|goal| goal.status != loreloom_core::GoalStatus::Active)
    {
        return Err("character.goals.status");
    }
    Ok(())
}

fn validate_references(
    definitions: &BTreeMap<ContentDefinitionId, RegisteredDefinition>,
) -> Result<(), ContentError> {
    for (owner, entry) in definitions {
        match &entry.definition {
            Definition::Resource(value) => {
                if let Some(attribute) = &value.derived_from_attribute {
                    require_kind(definitions, owner, attribute, "attribute")?;
                }
            }
            Definition::Condition(value) => {
                for tag in &value.tags {
                    require_kind(definitions, owner, tag, "tag")?;
                }
                validate_modifiers(definitions, owner, &value.modifiers)?;
                if let Some(periodic) = &value.periodic {
                    validate_effects(definitions, owner, &periodic.effects)?;
                }
            }
            Definition::Item(value) => {
                for tag in &value.tags {
                    require_kind(definitions, owner, tag, "tag")?;
                }
                validate_modifiers(definitions, owner, &value.modifiers)?;
                for slot in &value.equipment_slots {
                    require_kind(definitions, owner, slot, "equipment_slot")?;
                }
            }
            Definition::Skill(value) => {
                for cost in &value.costs {
                    require_kind(definitions, owner, &cost.resource_id, "resource")?;
                }
                validate_effects(definitions, owner, &value.effects)?;
                if let Some(reaction) = &value.reaction {
                    validate_predicates(definitions, owner, &reaction.predicates)?;
                }
            }
            Definition::Character(value) => {
                validate_character_references(definitions, owner, value)?;
            }
            Definition::Place(value) => {
                for tag in &value.tags {
                    require_kind(definitions, owner, tag, "tag")?;
                }
                for edge in &value.edges {
                    require_kind(definitions, owner, edge, "place")?;
                }
            }
            Definition::Scene(value) => {
                require_kind(definitions, owner, &value.entry_place, "place")?;
                for place in &value.places {
                    require_kind(definitions, owner, place, "place")?;
                }
                for character in &value.characters {
                    require_kind(definitions, owner, &character.character_id, "character")?;
                    require_kind(definitions, owner, &character.place_id, "place")?;
                    if !value.places.contains(&character.place_id) {
                        return Err(ContentError::InvalidValue {
                            id: owner.clone(),
                            field: "scene.character.place",
                        });
                    }
                    if character.controller == InitialCharacterController::Agent {
                        let has_agent_profile = definitions
                            .get(&character.character_id)
                            .and_then(|entry| match &entry.definition {
                                Definition::Character(character) => {
                                    character.agent_profile.as_ref()
                                }
                                _ => None,
                            })
                            .is_some();
                        if !has_agent_profile {
                            return Err(ContentError::InvalidValue {
                                id: owner.clone(),
                                field: "scene.character.agent_profile",
                            });
                        }
                    }
                }
            }
            Definition::EquipmentSlot(value) => {
                for tag in &value.allowed_item_tags {
                    require_kind(definitions, owner, tag, "tag")?;
                }
            }
            Definition::Parameter(value) => {
                validate_parameter_references(definitions, owner, value)?;
            }
            Definition::Event(value) => {
                for node in &value.nodes {
                    for option in &node.options {
                        validate_predicates(definitions, owner, &option.visible_if)?;
                        validate_predicates(definitions, owner, &option.enabled_if)?;
                        validate_effects(definitions, owner, &option.effects)?;
                    }
                }
            }
            Definition::GameplayAction(value) => {
                let mut parameter_ids = BTreeSet::new();
                for parameter in &value.parameters {
                    if !parameter_ids.insert(parameter.id.clone()) {
                        return Err(ContentError::InvalidValue {
                            id: owner.clone(),
                            field: "gameplay_action.parameters",
                        });
                    }
                    validate_parameter_type_references(definitions, owner, &parameter.value_type)?;
                }
                validate_predicates(definitions, owner, &value.predicates)?;
                validate_effects(definitions, owner, &value.effects)?;
            }
            Definition::Rule(value) => {
                match &value.trigger {
                    TriggerDefinition::SceneEntered { scene_id }
                    | TriggerDefinition::SceneLeft { scene_id } => {
                        require_kind(definitions, owner, scene_id, "scene")?;
                    }
                    TriggerDefinition::GameplayAction { action_id } => {
                        require_kind(definitions, owner, action_id, "gameplay_action")?;
                    }
                    TriggerDefinition::WorldEvent { .. } | TriggerDefinition::WorldClock { .. } => {
                    }
                }
                validate_predicates(definitions, owner, &value.predicates)?;
                validate_effects(definitions, owner, &value.effects)?;
            }
            Definition::AgentProfile(_)
            | Definition::Tag(_)
            | Definition::RelationshipKind(_)
            | Definition::Attribute(_) => {}
        }
    }
    Ok(())
}

fn validate_parameter_references(
    definitions: &BTreeMap<ContentDefinitionId, RegisteredDefinition>,
    owner: &ContentDefinitionId,
    parameter: &ParameterDefinition,
) -> Result<(), ContentError> {
    validate_parameter_type_references(definitions, owner, &parameter.value_type)
}

fn validate_parameter_type_references(
    definitions: &BTreeMap<ContentDefinitionId, RegisteredDefinition>,
    owner: &ContentDefinitionId,
    value_type: &ParameterType,
) -> Result<(), ContentError> {
    match value_type {
        ParameterType::TagSet { allowed, .. } => {
            for tag in allowed {
                require_kind(definitions, owner, tag, "tag")?;
            }
        }
        ParameterType::Enum { variants } => {
            if variants
                .iter()
                .any(|variant| variant.kind().ok() != Some("parameter_variant"))
            {
                return Err(ContentError::InvalidValue {
                    id: owner.clone(),
                    field: "parameter.enum.variants",
                });
            }
        }
        ParameterType::Bool
        | ParameterType::Fixed { .. }
        | ParameterType::Counter { .. }
        | ParameterType::ObjectRef { .. } => {}
    }
    Ok(())
}

fn validate_modifiers(
    definitions: &BTreeMap<ContentDefinitionId, RegisteredDefinition>,
    owner: &ContentDefinitionId,
    modifiers: &[crate::schema::ModifierDefinition],
) -> Result<(), ContentError> {
    for modifier in modifiers {
        require_kind(definitions, owner, &modifier.attribute_id, "attribute")?;
    }
    Ok(())
}

fn validate_effects(
    definitions: &BTreeMap<ContentDefinitionId, RegisteredDefinition>,
    owner: &ContentDefinitionId,
    effects: &[EffectDefinition],
) -> Result<(), ContentError> {
    for effect in effects {
        match effect {
            EffectDefinition::ResourceDelta { resource_id, .. } => {
                require_kind(definitions, owner, resource_id, "resource")?;
            }
            EffectDefinition::ApplyCondition { condition_id, .. } => {
                require_kind(definitions, owner, condition_id, "condition")?;
            }
            EffectDefinition::GrantItem { item_id, .. } => {
                require_kind(definitions, owner, item_id, "item")?;
            }
            EffectDefinition::GrantSkill { skill_id, .. } => {
                require_kind(definitions, owner, skill_id, "skill")?;
            }
            EffectDefinition::SetParameter { parameter_id, .. } => {
                require_kind(definitions, owner, parameter_id, "parameter")?;
            }
            EffectDefinition::EmitEvent { .. } => {}
        }
    }
    Ok(())
}

fn validate_predicates(
    definitions: &BTreeMap<ContentDefinitionId, RegisteredDefinition>,
    owner: &ContentDefinitionId,
    predicates: &[PredicateDefinition],
) -> Result<(), ContentError> {
    for predicate in predicates {
        match predicate {
            PredicateDefinition::ResourceAtLeast { resource_id, .. } => {
                require_kind(definitions, owner, resource_id, "resource")?;
            }
            PredicateDefinition::HasCondition { condition_id } => {
                require_kind(definitions, owner, condition_id, "condition")?;
            }
            PredicateDefinition::Not { predicate } => {
                validate_predicates(definitions, owner, std::slice::from_ref(predicate))?;
            }
            PredicateDefinition::All { predicates } | PredicateDefinition::Any { predicates } => {
                validate_predicates(definitions, owner, predicates)?;
            }
            PredicateDefinition::HasTag { tag_id } => {
                require_kind(definitions, owner, tag_id, "tag")?;
            }
        }
    }
    Ok(())
}

fn validate_character_references(
    definitions: &BTreeMap<ContentDefinitionId, RegisteredDefinition>,
    owner: &ContentDefinitionId,
    character: &CharacterDefinition,
) -> Result<(), ContentError> {
    if let Some(agent_profile) = &character.agent_profile {
        require_kind(definitions, owner, agent_profile, "agent_profile")?;
    }
    for tag in &character.profile.narrative_tags {
        require_kind(definitions, owner, tag, "tag")?;
    }
    for attribute in character.base_attributes.0.keys() {
        require_kind(definitions, owner, attribute, "attribute")?;
    }
    for attribute in character
        .spawn_constraints
        .minimum_attributes
        .keys()
        .chain(character.spawn_constraints.maximum_attributes.keys())
    {
        require_kind(definitions, owner, attribute, "attribute")?;
    }
    for definition_id in &character.spawn_constraints.allowed_definitions {
        require_any_kind(
            definitions,
            owner,
            definition_id,
            &["condition", "item", "skill"],
            "condition, item, or skill",
        )?;
    }
    for resource in &character.resources {
        require_kind(definitions, owner, &resource.resource_id, "resource")?;
    }
    for condition in &character.conditions {
        require_kind(definitions, owner, &condition.condition_id, "condition")?;
    }
    for item in &character.inventory {
        require_kind(definitions, owner, &item.item_id, "item")?;
    }
    for skill in &character.skills {
        require_kind(definitions, owner, &skill.skill_id, "skill")?;
    }
    for fact in &character.knowledge {
        require_kind(definitions, owner, &fact.predicate_id, "predicate")?;
    }
    Ok(())
}

fn require_any_kind(
    definitions: &BTreeMap<ContentDefinitionId, RegisteredDefinition>,
    owner: &ContentDefinitionId,
    target: &ContentDefinitionId,
    kinds: &[&str],
    expected: &'static str,
) -> Result<(), ContentError> {
    if definitions
        .get(target)
        .is_some_and(|entry| kinds.contains(&entry.definition.expected_kind()))
    {
        Ok(())
    } else {
        Err(ContentError::InvalidReference {
            owner: owner.clone(),
            target: target.clone(),
            expected,
        })
    }
}

fn require_kind(
    definitions: &BTreeMap<ContentDefinitionId, RegisteredDefinition>,
    owner: &ContentDefinitionId,
    target: &ContentDefinitionId,
    expected: &'static str,
) -> Result<(), ContentError> {
    if definitions
        .get(target)
        .is_some_and(|entry| entry.definition.expected_kind() == expected)
    {
        Ok(())
    } else {
        Err(ContentError::InvalidReference {
            owner: owner.clone(),
            target: target.clone(),
            expected,
        })
    }
}

pub fn parse_content_hash(value: impl Into<String>) -> Result<ContentHash, ContentError> {
    Ok(ContentHash::parse(value)?)
}
