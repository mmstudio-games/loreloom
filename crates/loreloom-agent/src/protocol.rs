use std::collections::{BTreeSet, HashSet};

use armillae_core::{CompletionRequest, ContentPart, Message, Role, ToolDefinition};
use loreloom_core::{
    ActorId, BoundedText, CharacterContext, ContentDefinitionId, EventId, LongText,
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
