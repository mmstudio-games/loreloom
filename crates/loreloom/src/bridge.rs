use std::sync::Mutex;

use armillae_core::{
    AssistantContent, CompletionRequest, CompletionResponse, ContentPart, FinishReason,
    TextContent, ToolCall, ToolCallId,
};
use armillae_llm::{
    BoxFuture, BridgeCapabilities, BridgeError, CompletionStream, LlmBridge, ProjectionReport,
};
use loreloom_agent::{
    AssignmentText, ClaimedActionText, NarrationText, NarratorPlan, NarratorSynthesis,
    NpcModelOutput, NpcTurnRequest, UtteranceText,
};
use loreloom_core::{ActorId, EventId, NpcTurnRequestId, ObjectId, Revision, SystemIdGenerator};
use serde_json::{Value as JsonValue, json};

pub struct DemoNarratorBridge {
    npc_id: ActorId,
    scene_id: ObjectId,
    ids: Mutex<SystemIdGenerator>,
}

impl DemoNarratorBridge {
    pub fn new(npc_id: ActorId, scene_id: ObjectId) -> Self {
        Self {
            npc_id,
            scene_id,
            ids: Mutex::new(SystemIdGenerator),
        }
    }
}

impl LlmBridge for DemoNarratorBridge {
    fn capabilities(&self) -> BridgeCapabilities {
        BridgeCapabilities::all()
    }

    fn project(&self, _request: &CompletionRequest) -> Result<ProjectionReport, BridgeError> {
        Ok(ProjectionReport::exact("loreloom-demo-narrator"))
    }

    fn complete<'a>(
        &'a self,
        request: CompletionRequest,
    ) -> BoxFuture<'a, Result<CompletionResponse, BridgeError>> {
        Box::pin(async move {
            let envelope = request_envelope(&request)?;
            match envelope["kind"].as_str() {
                Some("narrator_planning") => {
                    let revision = serde_json::from_value::<Revision>(
                        envelope["payload"]["observation"]["revision"].clone(),
                    )
                    .map_err(|_| invalid_request("invalid demo observation revision"))?;
                    let request_id = {
                        let mut ids = self
                            .ids
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        NpcTurnRequestId::generate_with(&mut *ids)
                            .map_err(|_| invalid_request("failed to allocate demo request id"))?
                    };
                    let plan = NarratorPlan {
                        based_on_revision: revision,
                        npc_turns: vec![NpcTurnRequest {
                            request_id,
                            actor_id: self.npc_id,
                            scene_id: self.scene_id,
                            based_on_revision: revision,
                            assignment: AssignmentText::new(
                                "Respond to the traveler, then let one world tick pass.",
                            )
                            .map_err(|_| invalid_request("invalid demo assignment"))?,
                        }],
                    };
                    json_response(&plan)
                }
                Some("narrator_synthesis") => {
                    let revision =
                        serde_json::from_value::<Revision>(envelope["payload"]["revision"].clone())
                            .map_err(|_| invalid_request("invalid demo synthesis revision"))?;
                    let supporting_events = envelope["payload"]["committed_events"]
                        .as_array()
                        .and_then(|events| events.last())
                        .and_then(|event| event.get("id"))
                        .cloned()
                        .map(serde_json::from_value::<EventId>)
                        .transpose()
                        .map_err(|_| invalid_request("invalid demo event id"))?
                        .into_iter()
                        .collect();
                    json_response(&NarratorSynthesis::Final {
                        based_on_revision: revision,
                        narration: NarrationText::new(
                            "Mira inclines her head. Rain measures one quiet moment against the shutters.",
                        )
                        .map_err(|_| invalid_request("invalid demo narration"))?,
                        supporting_events,
                    })
                }
                _ => Err(invalid_request("unknown demo narrator stage")),
            }
        })
    }

    fn stream<'a>(
        &'a self,
        _request: CompletionRequest,
    ) -> BoxFuture<'a, Result<CompletionStream, BridgeError>> {
        Box::pin(async { Err(invalid_request("demo bridge uses complete")) })
    }
}

#[derive(Debug, Default)]
pub struct DemoNpcBridge;

impl LlmBridge for DemoNpcBridge {
    fn capabilities(&self) -> BridgeCapabilities {
        BridgeCapabilities::all()
    }

    fn project(&self, _request: &CompletionRequest) -> Result<ProjectionReport, BridgeError> {
        Ok(ProjectionReport::exact("loreloom-demo-npc"))
    }

    fn complete<'a>(
        &'a self,
        request: CompletionRequest,
    ) -> BoxFuture<'a, Result<CompletionResponse, BridgeError>> {
        Box::pin(async move {
            let has_tool_result = request.messages.iter().any(|message| {
                message
                    .content
                    .iter()
                    .any(|part| matches!(part, ContentPart::ToolResult(_)))
            });
            if has_tool_result {
                json_response(&NpcModelOutput {
                    utterance: Some(
                        UtteranceText::new("The rain keeps honest time.")
                            .map_err(|_| invalid_request("invalid demo utterance"))?,
                    ),
                    intent: None,
                    claimed_action_description: Some(
                        ClaimedActionText::new("waited beside the hearth")
                            .map_err(|_| invalid_request("invalid demo claim"))?,
                    ),
                })
            } else {
                Ok(CompletionResponse {
                    id: None,
                    model: Some("loreloom-demo-npc".to_owned()),
                    content: vec![AssistantContent::ToolCall(ToolCall {
                        id: ToolCallId::new("demo-advance-time")
                            .map_err(|_| invalid_request("invalid demo tool id"))?,
                        name: "advance_time".to_owned(),
                        arguments: json!({ "ticks": 1 }),
                    })],
                    finish_reason: Some(FinishReason::ToolCall),
                    usage: None,
                    provider_metadata: JsonValue::Null,
                })
            }
        })
    }

    fn stream<'a>(
        &'a self,
        _request: CompletionRequest,
    ) -> BoxFuture<'a, Result<CompletionStream, BridgeError>> {
        Box::pin(async { Err(invalid_request("demo bridge uses complete")) })
    }
}

fn request_envelope(request: &CompletionRequest) -> Result<JsonValue, BridgeError> {
    request
        .messages
        .iter()
        .rev()
        .flat_map(|message| message.content.iter())
        .find_map(|part| match part {
            ContentPart::Text(text) => serde_json::from_str(&text.text).ok(),
            _ => None,
        })
        .ok_or_else(|| invalid_request("missing demo request envelope"))
}

fn json_response(value: &impl serde::Serialize) -> Result<CompletionResponse, BridgeError> {
    let text = serde_json::to_string(value)
        .map_err(|_| invalid_request("failed to encode demo response"))?;
    Ok(CompletionResponse {
        id: None,
        model: Some("loreloom-demo".to_owned()),
        content: vec![AssistantContent::Text(TextContent::new(text))],
        finish_reason: Some(FinishReason::Stop),
        usage: None,
        provider_metadata: JsonValue::Null,
    })
}

fn invalid_request(message: &'static str) -> BridgeError {
    BridgeError::InvalidRequest {
        message: message.to_owned(),
    }
}
