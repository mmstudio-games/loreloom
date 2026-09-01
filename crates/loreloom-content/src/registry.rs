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
    ParameterDefinition, ParameterPersistence, ParameterType, PlayerBootstrap,
    PlayerCreationBinding, PlayerCreationDraft, PlayerCreationEffect,
    PlayerCreationFieldDefinition, PlayerCreationFieldType, PlayerCreationFieldValue,
    PlayerCreationFormDefinition, PredicateDefinition, SkillKind, SkillTarget, TriggerDefinition,
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
pub struct CompiledPlayerCreation {
    pub character: CharacterSpawnSpec,
    pub parameter_overrides: BTreeMap<ContentDefinitionId, loreloom_core::ParameterValue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenePlaceSpawnPlan {
    pub definition_id: ContentDefinitionId,
    pub display_name: DisplayName,
    pub description: ShortText,
    pub tags: BTreeSet<ContentDefinitionId>,
    pub edges: BTreeSet<ContentDefinitionId>,
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
            for edge in &place.edges {
                if !scene.places.contains(edge) {
                    return Err(ContentError::CompileScene {
                        id: id.clone(),
                        reason: "place edge leaves the scene",
                    });
                }
                let Some(Definition::Place(peer)) = self
                    .definitions
                    .get(edge)
                    .map(|registered| &registered.definition)
                else {
                    return Err(ContentError::CompileScene {
                        id: id.clone(),
                        reason: "place edge is unavailable",
                    });
                };
                if edge == place_id || !peer.edges.contains(place_id) {
                    return Err(ContentError::CompileScene {
                        id: id.clone(),
                        reason: "place edge is not a bidirectional peer",
                    });
                }
            }
            places.push(ScenePlaceSpawnPlan {
                definition_id: place.id.clone(),
                display_name: place.display_name.clone(),
                description: place.description.clone(),
                tags: place.tags.clone(),
                edges: place.edges.clone(),
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

    pub fn compile_player(
        &self,
        default_character_id: &ContentDefinitionId,
        bootstrap: &PlayerBootstrap,
        request: CharacterCompileRequest,
    ) -> Result<CompiledPlayerCreation, ContentError> {
        let (mut character, parameter_overrides) = match bootstrap {
            PlayerBootstrap::Fixed => (
                self.compile_character(default_character_id, request)?,
                BTreeMap::new(),
            ),
            PlayerBootstrap::Preset { character_id } => (
                self.compile_character(character_id, request)?,
                BTreeMap::new(),
            ),
            PlayerBootstrap::Ugc { draft } => self.compile_player_draft(draft, request)?,
        };
        if character.controller != CharacterController::Player || character.agent_binding.is_some()
        {
            return Err(ContentError::CompileCharacter {
                id: default_character_id.clone(),
                reason: "player bootstrap must produce a player-controlled character",
            });
        }
        character.lifetime = CharacterLifetime::Persistent;
        Ok(CompiledPlayerCreation {
            character,
            parameter_overrides,
        })
    }

    fn compile_player_draft(
        &self,
        draft: &PlayerCreationDraft,
        request: CharacterCompileRequest,
    ) -> Result<
        (
            CharacterSpawnSpec,
            BTreeMap<ContentDefinitionId, loreloom_core::ParameterValue>,
        ),
        ContentError,
    > {
        let entry =
            self.definitions
                .get(&draft.form_id)
                .ok_or_else(|| ContentError::CompileCharacter {
                    id: draft.form_id.clone(),
                    reason: "player creation form is unavailable",
                })?;
        let Definition::PlayerCreationForm(form) = &entry.definition else {
            return Err(ContentError::CompileCharacter {
                id: draft.form_id.clone(),
                reason: "player creation definition has the wrong kind",
            });
        };
        if draft
            .values
            .keys()
            .any(|field_id| !form.fields.iter().any(|field| &field.id == field_id))
        {
            return Err(player_compile_error(
                form,
                "draft contains an unknown field",
            ));
        }

        let mut character = self.compile_character(&form.template, request)?;
        let EntityOrigin::Content { origin: template } = character.origin.clone() else {
            return Err(player_compile_error(
                form,
                "player template origin is invalid",
            ));
        };
        character.origin = EntityOrigin::PlayerCreated { template };
        let mut parameters = BTreeMap::new();
        let mut item_keys = character
            .inventory
            .iter()
            .map(|item| item.local_key.clone())
            .collect::<BTreeSet<_>>();
        let mut next_item = 0_u32;

        for field in &form.fields {
            let value = draft
                .values
                .get(&field.id)
                .cloned()
                .or_else(|| default_player_field_value(&field.value_type));
            let Some(value) = value else {
                if field.required {
                    return Err(player_compile_error(
                        form,
                        "required player field is missing",
                    ));
                }
                continue;
            };
            validate_player_field_value(form, field, &value)?;
            apply_player_binding(self, form, field, &value, &mut character, &mut parameters)?;
            apply_selected_player_effects(
                self,
                form,
                field,
                &value,
                &mut character,
                &mut parameters,
                &mut item_keys,
                &mut next_item,
            )?;
        }
        Ok((character, parameters))
    }

    pub fn compile_draft(
        &self,
        draft: &NpcDraft,
        policy: &GenerationPolicy,
        request: DraftCompileRequest,
    ) -> Result<CharacterSpawnSpec, ContentError> {
        validate_draft_references(&self.definitions, policy, draft)?;
        validate_draft_budget(policy, draft)?;
        let agent_binding = match request.controller {
            CharacterController::Agent => {
                let profile_id = policy
                    .allowed_agent_profiles
                    .iter()
                    .next()
                    .filter(|_| policy.allowed_agent_profiles.len() == 1)
                    .ok_or_else(|| ContentError::CompileCharacter {
                        id: policy.id.clone(),
                        reason: "GenerationPolicy must select exactly one AgentProfile",
                    })?;
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
            _ => None,
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

fn player_compile_error(form: &PlayerCreationFormDefinition, reason: &'static str) -> ContentError {
    ContentError::CompileCharacter {
        id: form.id.clone(),
        reason,
    }
}

fn default_player_field_value(
    value_type: &PlayerCreationFieldType,
) -> Option<PlayerCreationFieldValue> {
    match value_type {
        PlayerCreationFieldType::Text { default, .. } => default
            .as_ref()
            .and_then(|value| loreloom_core::LongText::new(value.as_str()).ok())
            .map(PlayerCreationFieldValue::Text),
        PlayerCreationFieldType::LongText { default, .. } => {
            default.clone().map(PlayerCreationFieldValue::Text)
        }
        PlayerCreationFieldType::Integer { default, .. } => {
            default.map(PlayerCreationFieldValue::Integer)
        }
        PlayerCreationFieldType::Number { default, .. } => {
            default.map(PlayerCreationFieldValue::Number)
        }
        PlayerCreationFieldType::Boolean { default } => {
            Some(PlayerCreationFieldValue::Boolean(*default))
        }
        PlayerCreationFieldType::SingleChoice { default, .. } => {
            default.clone().map(PlayerCreationFieldValue::SingleChoice)
        }
        PlayerCreationFieldType::MultiChoice { default, .. } => {
            default.clone().map(PlayerCreationFieldValue::MultiChoice)
        }
    }
}

fn validate_player_field_value(
    form: &PlayerCreationFormDefinition,
    field: &PlayerCreationFieldDefinition,
    value: &PlayerCreationFieldValue,
) -> Result<(), ContentError> {
    let valid = match (&field.value_type, value) {
        (
            PlayerCreationFieldType::Text {
                minimum_bytes,
                maximum_bytes,
                ..
            },
            PlayerCreationFieldValue::Text(value),
        )
        | (
            PlayerCreationFieldType::LongText {
                minimum_bytes,
                maximum_bytes,
                ..
            },
            PlayerCreationFieldValue::Text(value),
        ) => {
            let length = value.as_str().len();
            length >= *minimum_bytes as usize && length <= *maximum_bytes as usize
        }
        (
            PlayerCreationFieldType::Integer {
                minimum, maximum, ..
            },
            PlayerCreationFieldValue::Integer(value),
        ) => value >= minimum && value <= maximum,
        (
            PlayerCreationFieldType::Number {
                minimum, maximum, ..
            },
            PlayerCreationFieldValue::Number(value),
        ) => value >= minimum && value <= maximum,
        (PlayerCreationFieldType::Boolean { .. }, PlayerCreationFieldValue::Boolean(_)) => true,
        (
            PlayerCreationFieldType::SingleChoice { options, .. },
            PlayerCreationFieldValue::SingleChoice(value),
        ) => options.iter().any(|option| &option.value == value),
        (
            PlayerCreationFieldType::MultiChoice {
                minimum_selections,
                maximum_selections,
                options,
                ..
            },
            PlayerCreationFieldValue::MultiChoice(values),
        ) => {
            values.len() >= *minimum_selections as usize
                && values.len() <= *maximum_selections as usize
                && values
                    .iter()
                    .all(|value| options.iter().any(|option| &option.value == value))
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(player_compile_error(form, "player field value is invalid"))
    }
}

fn apply_player_binding(
    registry: &DefinitionRegistry,
    form: &PlayerCreationFormDefinition,
    field: &PlayerCreationFieldDefinition,
    value: &PlayerCreationFieldValue,
    character: &mut CharacterSpawnSpec,
    parameters: &mut BTreeMap<ContentDefinitionId, loreloom_core::ParameterValue>,
) -> Result<(), ContentError> {
    let Some(binding) = &field.binding else {
        return Ok(());
    };
    match (binding, value) {
        (PlayerCreationBinding::DisplayName, PlayerCreationFieldValue::Text(value)) => {
            character.display_name = DisplayName::non_empty(value.as_str())
                .map_err(|_| player_compile_error(form, "display name is invalid"))?;
        }
        (PlayerCreationBinding::ProfileSummary, PlayerCreationFieldValue::Text(value)) => {
            character.profile.summary = ShortText::new(value.as_str())
                .map_err(|_| player_compile_error(form, "profile summary is invalid"))?;
        }
        (PlayerCreationBinding::ProfileSpeakingStyle, PlayerCreationFieldValue::Text(value)) => {
            character.profile.speaking_style = ShortText::new(value.as_str())
                .map_err(|_| player_compile_error(form, "speaking style is invalid"))?;
        }
        (PlayerCreationBinding::ProfileValue, PlayerCreationFieldValue::Text(value)) => {
            character.profile.values.push(
                ShortText::new(value.as_str())
                    .map_err(|_| player_compile_error(form, "profile value is invalid"))?,
            );
        }
        (
            PlayerCreationBinding::Attribute { attribute_id },
            PlayerCreationFieldValue::Integer(value),
        ) => {
            let value = Fixed::from_integer(*value)
                .map_err(|_| player_compile_error(form, "attribute value overflows"))?;
            set_player_attribute(registry, form, character, attribute_id, value)?;
        }
        (
            PlayerCreationBinding::Attribute { attribute_id },
            PlayerCreationFieldValue::Number(value),
        ) => set_player_attribute(registry, form, character, attribute_id, *value)?,
        (
            PlayerCreationBinding::Parameter { parameter_id },
            PlayerCreationFieldValue::Integer(value),
        ) => set_player_parameter(
            registry,
            form,
            parameters,
            parameter_id,
            loreloom_core::ParameterValue::Counter(*value),
        )?,
        (
            PlayerCreationBinding::Parameter { parameter_id },
            PlayerCreationFieldValue::Number(value),
        ) => set_player_parameter(
            registry,
            form,
            parameters,
            parameter_id,
            loreloom_core::ParameterValue::Fixed(*value),
        )?,
        (
            PlayerCreationBinding::Parameter { parameter_id },
            PlayerCreationFieldValue::Boolean(value),
        ) => set_player_parameter(
            registry,
            form,
            parameters,
            parameter_id,
            loreloom_core::ParameterValue::Bool(*value),
        )?,
        (
            PlayerCreationBinding::Parameter { parameter_id },
            PlayerCreationFieldValue::SingleChoice(value),
        ) => set_player_parameter(
            registry,
            form,
            parameters,
            parameter_id,
            loreloom_core::ParameterValue::Enum(value.clone()),
        )?,
        (
            PlayerCreationBinding::Parameter { parameter_id },
            PlayerCreationFieldValue::MultiChoice(values),
        ) => set_player_parameter(
            registry,
            form,
            parameters,
            parameter_id,
            loreloom_core::ParameterValue::TagSet(values.clone()),
        )?,
        _ => {
            return Err(player_compile_error(
                form,
                "player field binding is incompatible",
            ));
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn apply_selected_player_effects(
    registry: &DefinitionRegistry,
    form: &PlayerCreationFormDefinition,
    field: &PlayerCreationFieldDefinition,
    value: &PlayerCreationFieldValue,
    character: &mut CharacterSpawnSpec,
    parameters: &mut BTreeMap<ContentDefinitionId, loreloom_core::ParameterValue>,
    item_keys: &mut BTreeSet<ShortText>,
    next_item: &mut u32,
) -> Result<(), ContentError> {
    let options = match (&field.value_type, value) {
        (
            PlayerCreationFieldType::SingleChoice { options, .. },
            PlayerCreationFieldValue::SingleChoice(selected),
        ) => options
            .iter()
            .filter(|option| &option.value == selected)
            .collect::<Vec<_>>(),
        (
            PlayerCreationFieldType::MultiChoice { options, .. },
            PlayerCreationFieldValue::MultiChoice(selected),
        ) => options
            .iter()
            .filter(|option| selected.contains(&option.value))
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    for option in options {
        for effect in &option.effects {
            apply_player_effect(
                registry, form, effect, character, parameters, item_keys, next_item,
            )?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn apply_player_effect(
    registry: &DefinitionRegistry,
    form: &PlayerCreationFormDefinition,
    effect: &PlayerCreationEffect,
    character: &mut CharacterSpawnSpec,
    parameters: &mut BTreeMap<ContentDefinitionId, loreloom_core::ParameterValue>,
    item_keys: &mut BTreeSet<ShortText>,
    next_item: &mut u32,
) -> Result<(), ContentError> {
    match effect {
        PlayerCreationEffect::GrantItem { item_id, quantity } => {
            require_kind(&registry.definitions, &form.id, item_id, "item")?;
            let local_key = loop {
                let candidate = ShortText::new(format!("ugc_item_{}", *next_item))
                    .map_err(|_| player_compile_error(form, "item key is invalid"))?;
                *next_item = next_item.saturating_add(1);
                if item_keys.insert(candidate.clone()) {
                    break candidate;
                }
            };
            character.inventory.push(ItemGrantInput {
                local_key,
                definition_id: item_id.clone(),
                quantity: *quantity,
                parent_local_key: None,
            });
        }
        PlayerCreationEffect::GrantSkill {
            skill_id,
            rank,
            proficiency,
        } => {
            require_kind(&registry.definitions, &form.id, skill_id, "skill")?;
            character.skills.push(SkillGrantInput {
                skill_id: skill_id.clone(),
                rank: rank.get(),
                proficiency: *proficiency,
                enabled: true,
            });
        }
        PlayerCreationEffect::ApplyCondition {
            condition_id,
            stacks,
            intensity,
        } => {
            require_kind(&registry.definitions, &form.id, condition_id, "condition")?;
            character.conditions.push(ConditionGrantInput {
                condition_id: condition_id.clone(),
                source: ConditionSource::System {
                    source_id: form.id.clone(),
                },
                stacks: *stacks,
                intensity: *intensity,
            });
        }
        PlayerCreationEffect::SetAttribute {
            attribute_id,
            value,
        } => set_player_attribute(registry, form, character, attribute_id, *value)?,
        PlayerCreationEffect::SetParameter {
            parameter_id,
            value,
        } => set_player_parameter(registry, form, parameters, parameter_id, value.clone())?,
        PlayerCreationEffect::AddNarrativeTag { tag_id } => {
            require_kind(&registry.definitions, &form.id, tag_id, "tag")?;
            character.profile.narrative_tags.insert(tag_id.clone());
        }
    }
    Ok(())
}

fn set_player_attribute(
    registry: &DefinitionRegistry,
    form: &PlayerCreationFormDefinition,
    character: &mut CharacterSpawnSpec,
    attribute_id: &ContentDefinitionId,
    value: Fixed,
) -> Result<(), ContentError> {
    require_kind(&registry.definitions, &form.id, attribute_id, "attribute")?;
    character.attributes.0.insert(attribute_id.clone(), value);
    Ok(())
}

fn set_player_parameter(
    registry: &DefinitionRegistry,
    form: &PlayerCreationFormDefinition,
    parameters: &mut BTreeMap<ContentDefinitionId, loreloom_core::ParameterValue>,
    parameter_id: &ContentDefinitionId,
    value: loreloom_core::ParameterValue,
) -> Result<(), ContentError> {
    let Some(Definition::Parameter(parameter)) = registry
        .definitions
        .get(parameter_id)
        .map(|entry| &entry.definition)
    else {
        return Err(player_compile_error(
            form,
            "player parameter is unavailable",
        ));
    };
    if parameter.persistence != ParameterPersistence::Save
        || !parameter_value_matches(&parameter.value_type, &value)
    {
        return Err(player_compile_error(
            form,
            "player parameter value is invalid",
        ));
    }
    parameters.insert(parameter_id.clone(), value);
    Ok(())
}

fn validate_draft_references(
    definitions: &BTreeMap<ContentDefinitionId, RegisteredDefinition>,
    policy: &GenerationPolicy,
    draft: &NpcDraft,
) -> Result<(), ContentError> {
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
        require_kind(definitions, &policy.id, &fact.predicate_id, "tag")?;
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
        Definition::PlayerCreationForm(value) => {
            validate_player_creation_form(value).map_err(invalid)
        }
        Definition::GenerationPolicy(value) if value.allowed_agent_profiles.len() != 1 => {
            Err(invalid("generation_policy.agent_profile"))
        }
        Definition::RelationshipKind(value) if value.minimum > value.maximum => {
            Err(invalid("relationship_kind.range"))
        }
        Definition::Parameter(value) => validate_parameter(value).map_err(invalid),
        Definition::Event(value) => validate_event(value).map_err(invalid),
        Definition::GameplayAction(value)
            if value.parameters.len() > 64
                || predicate_nodes(&value.predicates).is_none_or(|count| count > 64)
                || value.effects.len() > 32 =>
        {
            Err(invalid("gameplay_action.budget"))
        }
        Definition::Rule(value)
            if predicate_nodes(&value.predicates).is_none_or(|count| count > 64)
                || value.effects.len() > 32 =>
        {
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
    if parameter_value_matches(&value.value_type, &value.default) {
        Ok(())
    } else {
        Err("parameter.default")
    }
}

fn parameter_value_matches(
    value_type: &ParameterType,
    value: &loreloom_core::ParameterValue,
) -> bool {
    match (value_type, value) {
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
    }
}

fn validate_player_creation_form(form: &PlayerCreationFormDefinition) -> Result<(), &'static str> {
    if form.fields.is_empty() || form.fields.len() > 64 {
        return Err("player_creation.fields");
    }
    let mut field_ids = BTreeSet::new();
    for field in &form.fields {
        if field.id.kind().ok() != Some("player_field") || !field_ids.insert(field.id.clone()) {
            return Err("player_creation.field.id");
        }
        let binding_valid = matches!(
            (&field.value_type, &field.binding),
            (
                PlayerCreationFieldType::Text { .. } | PlayerCreationFieldType::LongText { .. },
                Some(
                    PlayerCreationBinding::DisplayName
                        | PlayerCreationBinding::ProfileSummary
                        | PlayerCreationBinding::ProfileSpeakingStyle
                        | PlayerCreationBinding::ProfileValue,
                ),
            ) | (
                PlayerCreationFieldType::Integer { .. } | PlayerCreationFieldType::Number { .. },
                Some(PlayerCreationBinding::Attribute { .. }),
            ) | (
                PlayerCreationFieldType::Integer { .. }
                    | PlayerCreationFieldType::Number { .. }
                    | PlayerCreationFieldType::Boolean { .. }
                    | PlayerCreationFieldType::SingleChoice { .. }
                    | PlayerCreationFieldType::MultiChoice { .. },
                Some(PlayerCreationBinding::Parameter { .. }),
            ) | (_, None)
        );
        if !binding_valid || !player_field_type_is_valid(&field.value_type) {
            return Err("player_creation.field.type");
        }
    }
    Ok(())
}

fn player_field_type_is_valid(value_type: &PlayerCreationFieldType) -> bool {
    match value_type {
        PlayerCreationFieldType::Text {
            minimum_bytes,
            maximum_bytes,
            default,
        } => {
            minimum_bytes <= maximum_bytes
                && *maximum_bytes <= 4_096
                && default.as_ref().is_none_or(|value| {
                    value.as_str().len() >= *minimum_bytes as usize
                        && value.as_str().len() <= *maximum_bytes as usize
                })
        }
        PlayerCreationFieldType::LongText {
            minimum_bytes,
            maximum_bytes,
            default,
        } => {
            minimum_bytes <= maximum_bytes
                && *maximum_bytes <= 65_536
                && default.as_ref().is_none_or(|value| {
                    value.as_str().len() >= *minimum_bytes as usize
                        && value.as_str().len() <= *maximum_bytes as usize
                })
        }
        PlayerCreationFieldType::Integer {
            minimum,
            maximum,
            default,
        } => {
            minimum <= maximum && default.is_none_or(|value| value >= *minimum && value <= *maximum)
        }
        PlayerCreationFieldType::Number {
            minimum,
            maximum,
            default,
        } => {
            minimum <= maximum && default.is_none_or(|value| value >= *minimum && value <= *maximum)
        }
        PlayerCreationFieldType::Boolean { .. } => true,
        PlayerCreationFieldType::SingleChoice { options, default } => {
            valid_player_choices(options)
                && default
                    .as_ref()
                    .is_none_or(|default| options.iter().any(|option| &option.value == default))
        }
        PlayerCreationFieldType::MultiChoice {
            minimum_selections,
            maximum_selections,
            options,
            default,
        } => {
            valid_player_choices(options)
                && minimum_selections <= maximum_selections
                && *maximum_selections <= options.len() as u32
                && default.as_ref().is_none_or(|default| {
                    default.len() >= *minimum_selections as usize
                        && default.len() <= *maximum_selections as usize
                        && default
                            .iter()
                            .all(|value| options.iter().any(|option| &option.value == value))
                })
        }
    }
}

fn valid_player_choices(options: &[crate::schema::PlayerCreationChoice]) -> bool {
    if options.is_empty() || options.len() > 64 {
        return false;
    }
    let mut values = BTreeSet::new();
    options
        .iter()
        .all(|option| values.insert(option.value.clone()) && option.effects.len() <= 32)
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
        if node.options.iter().any(|option| {
            predicate_nodes(&option.visible_if).is_none_or(|count| count > 64)
                || predicate_nodes(&option.enabled_if).is_none_or(|count| count > 64)
                || option.effects.len() > 32
        }) {
            return Err("event.option.budget");
        }
    }
    Ok(())
}

fn predicate_nodes(predicates: &[PredicateDefinition]) -> Option<usize> {
    predicates.iter().try_fold(0_usize, |total, predicate| {
        let children = match predicate {
            PredicateDefinition::Not { predicate } => {
                predicate_nodes(std::slice::from_ref(predicate))?
            }
            PredicateDefinition::All { predicates } | PredicateDefinition::Any { predicates } => {
                predicate_nodes(predicates)?
            }
            PredicateDefinition::ResourceAtLeast { .. }
            | PredicateDefinition::HasCondition { .. }
            | PredicateDefinition::HasTag { .. } => 0,
        };
        total.checked_add(1)?.checked_add(children)
    })
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
            Definition::PlayerCreationForm(value) => {
                validate_player_creation_references(definitions, owner, value)?;
            }
            Definition::GenerationPolicy(value) => {
                for profile in &value.allowed_agent_profiles {
                    require_kind(definitions, owner, profile, "agent_profile")?;
                }
                for attribute in value
                    .constraints
                    .minimum_attributes
                    .keys()
                    .chain(value.constraints.maximum_attributes.keys())
                {
                    require_kind(definitions, owner, attribute, "attribute")?;
                }
                for definition in &value.constraints.allowed_definitions {
                    if !definitions.contains_key(definition) {
                        return Err(ContentError::InvalidValue {
                            id: owner.clone(),
                            field: "generation_policy.allowed_definitions",
                        });
                    }
                }
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
    validate_rule_cycles(definitions)
}

fn validate_player_creation_references(
    definitions: &BTreeMap<ContentDefinitionId, RegisteredDefinition>,
    owner: &ContentDefinitionId,
    form: &PlayerCreationFormDefinition,
) -> Result<(), ContentError> {
    require_kind(definitions, owner, &form.template, "character")?;
    for field in &form.fields {
        match &field.binding {
            Some(PlayerCreationBinding::Attribute { attribute_id }) => {
                require_kind(definitions, owner, attribute_id, "attribute")?;
            }
            Some(PlayerCreationBinding::Parameter { parameter_id }) => {
                require_kind(definitions, owner, parameter_id, "parameter")?;
                let Some(Definition::Parameter(parameter)) =
                    definitions.get(parameter_id).map(|entry| &entry.definition)
                else {
                    return Err(ContentError::InvalidValue {
                        id: owner.clone(),
                        field: "player_creation.parameter",
                    });
                };
                if parameter.persistence != ParameterPersistence::Save
                    || !player_field_matches_parameter(&field.value_type, &parameter.value_type)
                {
                    return Err(ContentError::InvalidValue {
                        id: owner.clone(),
                        field: "player_creation.parameter",
                    });
                }
            }
            Some(
                PlayerCreationBinding::DisplayName
                | PlayerCreationBinding::ProfileSummary
                | PlayerCreationBinding::ProfileSpeakingStyle
                | PlayerCreationBinding::ProfileValue,
            )
            | None => {}
        }
        for effect in player_field_effects(&field.value_type) {
            match effect {
                PlayerCreationEffect::GrantItem { item_id, .. } => {
                    require_kind(definitions, owner, item_id, "item")?;
                }
                PlayerCreationEffect::GrantSkill { skill_id, .. } => {
                    require_kind(definitions, owner, skill_id, "skill")?;
                }
                PlayerCreationEffect::ApplyCondition { condition_id, .. } => {
                    require_kind(definitions, owner, condition_id, "condition")?;
                }
                PlayerCreationEffect::SetAttribute { attribute_id, .. } => {
                    require_kind(definitions, owner, attribute_id, "attribute")?;
                }
                PlayerCreationEffect::SetParameter {
                    parameter_id,
                    value,
                } => {
                    require_kind(definitions, owner, parameter_id, "parameter")?;
                    let Some(Definition::Parameter(parameter)) =
                        definitions.get(parameter_id).map(|entry| &entry.definition)
                    else {
                        return Err(ContentError::InvalidValue {
                            id: owner.clone(),
                            field: "player_creation.effect.parameter",
                        });
                    };
                    if parameter.persistence != ParameterPersistence::Save
                        || !parameter_value_matches(&parameter.value_type, value)
                    {
                        return Err(ContentError::InvalidValue {
                            id: owner.clone(),
                            field: "player_creation.effect.parameter",
                        });
                    }
                }
                PlayerCreationEffect::AddNarrativeTag { tag_id } => {
                    require_kind(definitions, owner, tag_id, "tag")?;
                }
            }
        }
    }
    Ok(())
}

fn player_field_matches_parameter(
    field: &PlayerCreationFieldType,
    parameter: &ParameterType,
) -> bool {
    match (field, parameter) {
        (PlayerCreationFieldType::Integer { .. }, ParameterType::Counter { .. })
        | (PlayerCreationFieldType::Number { .. }, ParameterType::Fixed { .. })
        | (PlayerCreationFieldType::Boolean { .. }, ParameterType::Bool) => true,
        (
            PlayerCreationFieldType::SingleChoice { options, .. },
            ParameterType::Enum { variants },
        ) => options
            .iter()
            .all(|option| variants.contains(&option.value)),
        (
            PlayerCreationFieldType::MultiChoice { options, .. },
            ParameterType::TagSet { allowed, maximum },
        ) => {
            options.iter().all(|option| allowed.contains(&option.value))
                && options.len() <= *maximum as usize
        }
        _ => false,
    }
}

fn player_field_effects(
    field: &PlayerCreationFieldType,
) -> impl Iterator<Item = &PlayerCreationEffect> {
    match field {
        PlayerCreationFieldType::SingleChoice { options, .. }
        | PlayerCreationFieldType::MultiChoice { options, .. } => Some(options),
        _ => None,
    }
    .into_iter()
    .flatten()
    .flat_map(|option| option.effects.iter())
}

fn validate_rule_cycles(
    definitions: &BTreeMap<ContentDefinitionId, RegisteredDefinition>,
) -> Result<(), ContentError> {
    let mut listeners = BTreeMap::<ShortText, Vec<ContentDefinitionId>>::new();
    for (id, entry) in definitions {
        if let Definition::Rule(rule) = &entry.definition
            && let TriggerDefinition::WorldEvent { event_type } = &rule.trigger
        {
            listeners
                .entry(event_type.clone())
                .or_default()
                .push(id.clone());
        }
    }
    let mut edges = BTreeMap::<ContentDefinitionId, BTreeSet<ContentDefinitionId>>::new();
    for (id, entry) in definitions {
        let Definition::Rule(rule) = &entry.definition else {
            continue;
        };
        for effect in &rule.effects {
            let event_type = effect_event_type(effect)?;
            if let Some(targets) = event_type.as_ref().and_then(|kind| listeners.get(kind)) {
                edges
                    .entry(id.clone())
                    .or_default()
                    .extend(targets.iter().cloned());
            }
        }
    }
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for id in edges.keys() {
        if rule_cycle_from(id, &edges, &mut visiting, &mut visited) {
            return Err(ContentError::InvalidValue {
                id: id.clone(),
                field: "rule.emit_cycle",
            });
        }
    }
    Ok(())
}

fn effect_event_type(effect: &EffectDefinition) -> Result<Option<ShortText>, ContentError> {
    let value = match effect {
        EffectDefinition::ResourceDelta { .. } => Some("resource_changed"),
        EffectDefinition::ApplyCondition { .. } => Some("condition_applied"),
        EffectDefinition::GrantItem { .. } => Some("item_granted"),
        EffectDefinition::GrantSkill { .. } => Some("skill_granted"),
        EffectDefinition::SetParameter { .. } => Some("parameter_changed"),
        EffectDefinition::EmitEvent { event_type } => return Ok(Some(event_type.clone())),
    };
    value
        .map(ShortText::new)
        .transpose()
        .map_err(|_| ContentError::TextBound)
}

fn rule_cycle_from(
    id: &ContentDefinitionId,
    edges: &BTreeMap<ContentDefinitionId, BTreeSet<ContentDefinitionId>>,
    visiting: &mut BTreeSet<ContentDefinitionId>,
    visited: &mut BTreeSet<ContentDefinitionId>,
) -> bool {
    if visited.contains(id) {
        return false;
    }
    if !visiting.insert(id.clone()) {
        return true;
    }
    if edges.get(id).is_some_and(|targets| {
        targets
            .iter()
            .any(|target| rule_cycle_from(target, edges, visiting, visited))
    }) {
        return true;
    }
    visiting.remove(id);
    visited.insert(id.clone());
    false
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
        require_kind(definitions, owner, &fact.predicate_id, "tag")?;
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
