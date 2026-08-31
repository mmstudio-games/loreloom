use std::collections::{BTreeSet, HashSet};

use armillae_core::{CompletionRequest, ContentPart, Message, Role, ToolDefinition};
use loreloom_core::{
    ActorId, BoundedText, CharacterContext, ContentDefinitionId, DisplayName, EventId, LongText,
    NpcTurnRequestId, ObjectId, Revision, SceneContext, ShortText, TranscriptItemRecord,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;

use crate::BudgetReason;

pub type AssignmentText = BoundedText<4096>;
pub type UtteranceText = BoundedText<16384>;
pub type IntentText = BoundedText<4096>;
pub type ClaimedActionText = BoundedText<8192>;
pub type NarrationText = BoundedText<65536>;

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("agent context revisions do not match")]
    ContextRevision,
    #[error("narrator plan is invalid: {field}")]
    InvalidPlan { field: &'static str },
    #[error("narrator NPC decision is invalid: {field}")]
    InvalidNpcDecision { field: &'static str },
    #[error("agent context JSON encoding failed")]
    ContextEncoding(#[source] serde_json::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentDefinition {
    pub profile_id: ContentDefinitionId,
    pub system_style: LongText,
    pub model_alias: ShortText,
    pub allowed_tools: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NpcAssignment {
    pub text: AssignmentText,
    pub revision: Revision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NpcGenerationRequest {
    pub scene_id: ObjectId,
    pub role: ShortText,
    pub purpose: LongText,
    pub desired_traits: BTreeSet<ContentDefinitionId>,
    pub importance: NarrativeImportance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NarrativeImportance {
    Ambient,
    Supporting,
    Principal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum NpcTarget {
    Existing {
        actor_id: ActorId,
    },
    Preset {
        character_id: ContentDefinitionId,
        place_id: ObjectId,
    },
    Generated {
        generation_policy_id: ContentDefinitionId,
        place_id: ObjectId,
        request: NpcGenerationRequest,
    },
    Mentioned {
        display_name: DisplayName,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NpcNarrativeAction {
    MentionOnly,
    MaterializeLightweight,
    RequestNpcTurn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NpcLifetime {
    Beat,
    Scene,
    Persistent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "profile_id",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum NpcControllerKind {
    NarratorProxy,
    Rules,
    Agent(ContentDefinitionId),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NarratorNpcDecision {
    pub target: NpcTarget,
    pub action: NpcNarrativeAction,
    pub lifetime: NpcLifetime,
    pub controller: NpcControllerKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignment: Option<NpcAssignment>,
}

impl NarratorNpcDecision {
    pub fn validate(&self) -> Result<(), AgentError> {
        if self.lifetime == NpcLifetime::Beat && self.action != NpcNarrativeAction::MentionOnly {
            return Err(AgentError::InvalidNpcDecision {
                field: "beat_requires_mention_only",
            });
        }
        if matches!(self.target, NpcTarget::Mentioned { .. })
            && self.action != NpcNarrativeAction::MentionOnly
        {
            return Err(AgentError::InvalidNpcDecision {
                field: "mentioned_requires_mention_only",
            });
        }
        if matches!(self.target, NpcTarget::Existing { .. })
            && self.action == NpcNarrativeAction::MaterializeLightweight
        {
            return Err(AgentError::InvalidNpcDecision {
                field: "existing_is_already_materialized",
            });
        }
        match self.action {
            NpcNarrativeAction::MentionOnly => {
                if self.assignment.is_some() {
                    return Err(AgentError::InvalidNpcDecision {
                        field: "mention_has_no_assignment",
                    });
                }
            }
            NpcNarrativeAction::MaterializeLightweight => {
                if matches!(self.controller, NpcControllerKind::Agent(_)) {
                    return Err(AgentError::InvalidNpcDecision {
                        field: "lightweight_controller",
                    });
                }
                if self.assignment.is_some() {
                    return Err(AgentError::InvalidNpcDecision {
                        field: "lightweight_has_no_assignment",
                    });
                }
            }
            NpcNarrativeAction::RequestNpcTurn => {
                if !matches!(self.controller, NpcControllerKind::Agent(_)) {
                    return Err(AgentError::InvalidNpcDecision {
                        field: "npc_turn_requires_agent",
                    });
                }
            }
        }
        if self.requires_materialization() && self.assignment.is_some() {
            return Err(AgentError::InvalidNpcDecision {
                field: "assignment_requires_materialized_actor",
            });
        }
        Ok(())
    }

    #[must_use]
    pub fn requires_materialization(&self) -> bool {
        !matches!(self.action, NpcNarrativeAction::MentionOnly)
            && matches!(
                self.target,
                NpcTarget::Preset { .. } | NpcTarget::Generated { .. }
            )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NpcContext {
    pub actor_id: ActorId,
    pub revision: Revision,
    pub character: CharacterContext,
    pub scene: SceneContext,
    pub assignment: NpcAssignment,
    pub recent_dialogue: Vec<TranscriptItemRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NpcAgent {
    pub definition: AgentDefinition,
    pub context: NpcContext,
}

impl NpcAgent {
    pub fn new(
        definition: AgentDefinition,
        character: CharacterContext,
        scene: SceneContext,
        assignment: NpcAssignment,
        recent_dialogue: Vec<TranscriptItemRecord>,
    ) -> Result<Self, AgentError> {
        if character.revision != scene.revision || character.revision != assignment.revision {
            return Err(AgentError::ContextRevision);
        }
        Ok(Self {
            context: NpcContext {
                actor_id: character.actor_id,
                revision: character.revision,
                character,
                scene,
                assignment,
                recent_dialogue,
            },
            definition,
        })
    }

    pub fn request(
        &self,
        definitions: impl IntoIterator<Item = ToolDefinition>,
    ) -> Result<CompletionRequest, AgentError> {
        let allowed = &self.definition.allowed_tools;
        let tools = definitions
            .into_iter()
            .filter(|definition| allowed.contains(&definition.name))
            .collect();
        let context = serde_json::to_string(&json!({
            "kind": "npc_turn",
            "profile_id": self.definition.profile_id,
            "context": self.context,
            "output_contract": {
                "utterance": "optional string",
                "intent": "optional string",
                "claimed_action_description": "optional string"
            }
        }))
        .map_err(AgentError::ContextEncoding)?;
        Ok(CompletionRequest {
            messages: vec![
                Message::new(
                    Role::System,
                    vec![ContentPart::text(
                        "Follow product safety and tool rules. Claims are not world facts; use tools for state changes.",
                    )],
                ),
                Message::new(
                    Role::System,
                    vec![ContentPart::text(self.definition.system_style.as_str())],
                ),
                Message::user(context),
            ],
            tools,
            ..CompletionRequest::default()
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NarratorPlan {
    pub based_on_revision: Revision,
    pub npc_turns: Vec<NpcTurnRequest>,
}

impl NarratorPlan {
    pub fn validate(&self) -> Result<(), AgentError> {
        let mut ids = HashSet::new();
        for request in &self.npc_turns {
            if request.based_on_revision != self.based_on_revision {
                return Err(AgentError::InvalidPlan {
                    field: "request_revision",
                });
            }
            if !ids.insert(request.request_id) {
                return Err(AgentError::InvalidPlan {
                    field: "duplicate_request_id",
                });
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NpcTurnRequest {
    pub request_id: NpcTurnRequestId,
    pub actor_id: ActorId,
    pub scene_id: ObjectId,
    pub based_on_revision: Revision,
    pub assignment: AssignmentText,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "reason", rename_all = "snake_case")]
pub enum NpcTurnStatus {
    Completed,
    Stale,
    Rejected,
    Cancelled,
    BudgetExhausted(BudgetReason),
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NpcTurnResult {
    pub request_id: NpcTurnRequestId,
    pub actor_id: ActorId,
    pub observed_revision: Option<Revision>,
    pub final_revision: Revision,
    pub status: NpcTurnStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub utterance: Option<UtteranceText>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<IntentText>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_action_description: Option<ClaimedActionText>,
    pub tool_call_ids: Vec<String>,
    pub world_events: Vec<EventId>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NpcModelOutput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub utterance: Option<UtteranceText>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<IntentText>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_action_description: Option<ClaimedActionText>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NarratorSynthesis {
    Final {
        based_on_revision: Revision,
        narration: NarrationText,
        supporting_events: Vec<EventId>,
    },
    Continue {
        based_on_revision: Revision,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        narration: Option<NarrationText>,
        supporting_events: Vec<EventId>,
        next_plan: NarratorPlan,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object_id() -> ObjectId {
        "obj_01890f6a-2b3e-7d4e-8f90-123456789abc"
            .parse()
            .expect("object ID")
    }

    fn profile_id() -> ContentDefinitionId {
        "games.loreloom.test:agent_profile/keeper"
            .parse()
            .expect("profile ID")
    }

    #[test]
    fn generated_npc_decision_round_trips_strictly() {
        let decision = NarratorNpcDecision {
            target: NpcTarget::Generated {
                generation_policy_id: "games.loreloom.test:generation_policy/default"
                    .parse()
                    .expect("policy ID"),
                place_id: object_id(),
                request: NpcGenerationRequest {
                    scene_id: object_id(),
                    role: ShortText::new("innkeeper").expect("role"),
                    purpose: LongText::new("Answer the traveler without inventing world facts.")
                        .expect("purpose"),
                    desired_traits: BTreeSet::new(),
                    importance: NarrativeImportance::Supporting,
                },
            },
            action: NpcNarrativeAction::RequestNpcTurn,
            lifetime: NpcLifetime::Scene,
            controller: NpcControllerKind::Agent(profile_id()),
            assignment: None,
        };

        decision.validate().expect("valid decision");
        let encoded = serde_json::to_value(&decision).expect("encode decision");
        assert_eq!(
            serde_json::from_value::<NarratorNpcDecision>(encoded.clone())
                .expect("decode decision"),
            decision
        );
        let mut unknown = encoded.as_object().expect("decision object").clone();
        unknown.insert("priority".to_owned(), serde_json::json!(999));
        assert!(
            serde_json::from_value::<NarratorNpcDecision>(serde_json::Value::Object(unknown))
                .is_err()
        );
    }

    #[test]
    fn invalid_lifetime_and_controller_combinations_are_rejected() {
        let lightweight_agent = NarratorNpcDecision {
            target: NpcTarget::Preset {
                character_id: "games.loreloom.test:character/keeper"
                    .parse()
                    .expect("character ID"),
                place_id: object_id(),
            },
            action: NpcNarrativeAction::MaterializeLightweight,
            lifetime: NpcLifetime::Scene,
            controller: NpcControllerKind::Agent(profile_id()),
            assignment: None,
        };
        assert!(lightweight_agent.validate().is_err());

        let beat_entity = NarratorNpcDecision {
            controller: NpcControllerKind::NarratorProxy,
            lifetime: NpcLifetime::Beat,
            ..lightweight_agent
        };
        assert!(beat_entity.validate().is_err());
    }
}
