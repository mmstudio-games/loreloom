use serde::{Deserialize, Serialize};

use crate::{
    ActionState, ActorId, CharacterController, CharacterProfile, ConditionRecord,
    ContentDefinitionId, DisplayName, EventId, Fixed, GoalRecord, ItemRecord, KnownFactRecord,
    LifeState, ObjectId, ParameterValue, Posture, Revision, SessionId, ShortText, SkillGrantRecord,
    TranscriptItemId, TranscriptItemRecord, WorldEvent, WorldTime,
};

pub const DIAGNOSED_CONDITION_PREDICATE_ID: &str = "games.loreloom.core:tag/diagnosed_condition";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttributeView {
    pub attribute_id: ContentDefinitionId,
    pub display_name: DisplayName,
    pub base: Fixed,
    pub effective: Fixed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceView {
    pub resource_id: ContentDefinitionId,
    pub display_name: DisplayName,
    pub current: Fixed,
    pub maximum: Fixed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionView {
    pub condition: ConditionRecord,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<DisplayName>,
    pub symptoms: Vec<ShortText>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InventoryView {
    pub item: ItemRecord,
    pub display_name: DisplayName,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillView {
    pub grant: SkillGrantRecord,
    pub display_name: DisplayName,
    pub available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CharacterContext {
    pub actor_id: ActorId,
    pub revision: Revision,
    pub display_name: DisplayName,
    pub profile: CharacterProfile,
    pub location_id: ObjectId,
    pub attributes: Vec<AttributeView>,
    pub resources: Vec<ResourceView>,
    pub conditions: Vec<ConditionView>,
    pub inventory: Vec<InventoryView>,
    pub skills: Vec<SkillView>,
    pub known_facts: Vec<KnownFactRecord>,
    pub goals: Vec<GoalRecord>,
    pub life_state: LifeState,
    pub action_state: ActionState,
    pub posture: Posture,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VisibleActorView {
    pub actor_id: ActorId,
    pub display_name: DisplayName,
    pub controller: CharacterController,
    pub npc_turn_available: bool,
    pub life_state: LifeState,
    pub action_state: ActionState,
    pub posture: Posture,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdjacentPlaceView {
    pub place_id: ObjectId,
    pub display_name: DisplayName,
    pub description: ShortText,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneContext {
    pub scene_id: ObjectId,
    pub revision: Revision,
    pub display_name: DisplayName,
    pub framing: ShortText,
    pub place_id: ObjectId,
    pub place_name: DisplayName,
    pub adjacent_places: Vec<AdjacentPlaceView>,
    pub clock: WorldTime,
    pub visible_actors: Vec<VisibleActorView>,
    pub recent_events: Vec<WorldEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneObservation {
    pub revision: Revision,
    pub session_id: SessionId,
    pub player: CharacterContext,
    pub scene: SceneContext,
    pub recent_transcript: Vec<TranscriptItemRecord>,
    pub player_input: crate::LongText,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimePhase {
    Idle,
    PersistingInput,
    NarratorThinking,
    ResolvingOrchestration,
    NpcThinking,
    UpdatingWorld,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolActivityState {
    Pending,
    Succeeded,
    Rejected,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolActivity {
    pub call_id: String,
    pub name: String,
    pub state: ToolActivityState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeKind {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiNotice {
    pub kind: NoticeKind,
    pub message: ShortText,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterValueView {
    pub parameter_id: ContentDefinitionId,
    pub display_name: DisplayName,
    pub value: ParameterValue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterSetView {
    pub set_id: ObjectId,
    pub schema_id: ContentDefinitionId,
    pub values: Vec<ParameterValueView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventOptionView {
    pub option_id: ContentDefinitionId,
    pub display_name: DisplayName,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActiveEventView {
    pub event_id: ObjectId,
    pub definition_id: ContentDefinitionId,
    pub display_name: DisplayName,
    pub current_node: ContentDefinitionId,
    pub node_text: ShortText,
    pub options: Vec<EventOptionView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptWindow {
    pub items: Vec<TranscriptItemRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_cursor: Option<TranscriptItemId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiSnapshot {
    pub revision: Revision,
    pub session_id: SessionId,
    pub player: CharacterContext,
    pub scene: SceneContext,
    pub parameters: Vec<ParameterSetView>,
    pub active_events: Vec<ActiveEventView>,
    pub transcript: TranscriptWindow,
    pub tool_activity: Vec<ToolActivity>,
    pub phase: RuntimePhase,
    pub can_submit: bool,
    pub can_cancel: bool,
    pub waiting: bool,
    pub notices: Vec<UiNotice>,
    pub supporting_events: Vec<EventId>,
}
