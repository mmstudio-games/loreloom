use armillae_core::{
    AssistantContent, CompletionRequest, CompletionResponse, ContentPart, FinishReason,
    TextContent, ToolCall, ToolCallId,
};
use armillae_llm::{
    BoxFuture, BridgeCapabilities, BridgeError, CompletionStream, LlmBridge, ProjectionReport,
};
use loreloom_core::{ActorId, ObjectId};
use serde_json::{Value as JsonValue, json};

pub struct DemoNarratorBridge {
    npc_id: ActorId,
    scene_id: ObjectId,
}

impl DemoNarratorBridge {
    pub fn new(npc_id: ActorId, scene_id: ObjectId) -> Self {
        Self { npc_id, scene_id }
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
                Some("narrator_turn") => {
                    let has_npc_results = envelope["payload"]["npc_results"]
                        .as_array()
                        .is_some_and(|results| !results.is_empty());
                    let has_tool_result = request.messages.iter().any(|message| {
                        message
                            .content
                            .iter()
                            .any(|part| matches!(part, ContentPart::ToolResult(_)))
                    });
                    if has_npc_results {
                        Ok(text_response(
                            "Mira inclines her head. Rain measures one quiet moment against the shutters.",
                        ))
                    } else if has_tool_result {
                        Ok(text_response("Mira considers the traveler."))
                    } else {
                        Ok(CompletionResponse {
                            id: None,
                            model: Some("loreloom-demo-narrator".to_owned()),
                            content: vec![AssistantContent::ToolCall(ToolCall {
                                id: ToolCallId::new("demo-request-npc")
                                    .map_err(|_| invalid_request("invalid demo tool id"))?,
                                name: "request_npc_turn".to_owned(),
                                arguments: json!({
                                    "actor_id": self.npc_id,
                                    "scene_id": self.scene_id,
                                    "assignment": "Respond to the traveler, then let one world tick pass."
                                }),
                            })],
                            finish_reason: Some(FinishReason::ToolCall),
                            usage: None,
                            provider_metadata: JsonValue::Null,
                        })
                    }
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
                Ok(text_response(
                    "The rain keeps honest time. I wait beside the hearth.",
                ))
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

fn text_response(text: impl Into<String>) -> CompletionResponse {
    CompletionResponse {
        id: None,
        model: Some("loreloom-demo".to_owned()),
        content: vec![AssistantContent::Text(TextContent::new(text.into()))],
        finish_reason: Some(FinishReason::Stop),
        usage: None,
        provider_metadata: JsonValue::Null,
    }
}

fn invalid_request(message: &'static str) -> BridgeError {
    BridgeError::InvalidRequest {
        message: message.to_owned(),
    }
}
