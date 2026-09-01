use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    num::NonZeroU32,
};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;

use crate::{
    ActionId, ActorId, ContentDefinitionId, DisplayName, EventId, Fixed, GenerationId, LongText,
    ModId, ObjectId, Revision, SessionId, ShortText, TranscriptItemId, WorldId, WorldTime,
};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DomainValueError {
    #[error("content hash must be exactly 64 lowercase hexadecimal bytes")]
    InvalidContentHash,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContentHash(String);

impl ContentHash {
    pub fn parse(value: impl Into<String>) -> Result<Self, DomainValueError> {
        let value = value.into();
        if value.len() != 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(DomainValueError::InvalidContentHash);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ContentHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for ContentHash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ContentHash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::parse(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentOrigin {
    pub mod_id: ModId,
    pub mod_version: ShortText,
    pub pack_id: ContentDefinitionId,
    pub definition_id: ContentDefinitionId,
    pub content_version: u32,
    pub content_hash: ContentHash,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratedOrigin {
    pub generation_id: GenerationId,
    pub generator_version: ShortText,
    pub source: GenerationSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum GenerationSource {
    PlayerInput { transcript_id: TranscriptItemId },
    WorldEvent { event_id: EventId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum EntityOrigin {
    Content { origin: ContentOrigin },
    Generated { origin: GeneratedOrigin },
    System { source: ContentDefinitionId },
}

impl EntityOrigin {
    #[must_use]
    pub const fn content(&self) -> Option<&ContentOrigin> {
        match self {
            Self::Content { origin } => Some(origin),
            Self::Generated { .. } | Self::System { .. } => None,
        }
    }
}

pub type CharacterOrigin = EntityOrigin;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutonomyMode {
    Directed,
    Reactive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentBinding {
    pub profile_id: ContentDefinitionId,
    pub enabled: bool,
    pub autonomy: AutonomyMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CharacterController {
    Player,
    NarratorProxy,
    Rules,
    Agent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum CharacterLifetime {
    Scene { scene_id: ObjectId },
    Persistent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CharacterProfile {
    pub summary: ShortText,
    pub values: Vec<ShortText>,
    pub speaking_style: ShortText,
    pub narrative_tags: BTreeSet<ContentDefinitionId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BaseAttributes(pub BTreeMap<ContentDefinitionId, Fixed>);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttributeOperation {
    Flat,
    Multiply,
    Override,
    ClampMinimum,
    ClampMaximum,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttributeAdjustment {
    pub source_id: ObjectId,
    pub attribute_id: ContentDefinitionId,
    pub operation: AttributeOperation,
    pub value: Fixed,
    pub priority: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifeState {
    Alive,
    Downed,
    Dead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ActionState {
    Idle,
    Acting { action_id: ActionId },
    Waiting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Posture {
    Standing,
    Sitting,
    Prone,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourcePool {
    pub resource_id: ContentDefinitionId,
    pub current: Fixed,
    pub base_maximum: Fixed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldStateRecord {
    pub id: WorldId,
    pub player_actor: ActorId,
    pub active_scene: ObjectId,
    pub clock: WorldTime,
    pub rng_seed: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneRecord {
    pub id: ObjectId,
    pub display_name: DisplayName,
    pub framing: ShortText,
    pub entry_place: ObjectId,
    pub active: bool,
    pub origin: EntityOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaceRecord {
    pub id: ObjectId,
    pub scene_id: ObjectId,
    pub display_name: DisplayName,
    pub description: ShortText,
    pub tags: BTreeSet<ContentDefinitionId>,
    pub edges: BTreeSet<ObjectId>,
    pub origin: EntityOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CharacterRecord {
    pub id: ActorId,
    pub display_name: DisplayName,
    pub profile: CharacterProfile,
    pub controller: CharacterController,
    pub lifetime: CharacterLifetime,
    pub location: ObjectId,
    pub inventory_root: ObjectId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_binding: Option<AgentBinding>,
    pub base_attributes: BaseAttributes,
    pub attribute_adjustments: Vec<AttributeAdjustment>,
    pub resources: BTreeMap<ContentDefinitionId, ResourcePool>,
    pub life_state: LifeState,
    pub action_state: ActionState,
    pub posture: Posture,
    pub origin: CharacterOrigin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StackState(pub NonZeroU32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Durability {
    pub current: Fixed,
    pub maximum: Fixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContainerState {
    pub max_weight_grams: Fixed,
    pub max_children: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EquippedState {
    pub wearer_id: ActorId,
    pub slot_id: ContentDefinitionId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ParameterValue {
    Bool(bool),
    Fixed(Fixed),
    Counter(i64),
    Enum(ContentDefinitionId),
    TagSet(BTreeSet<ContentDefinitionId>),
    ObjectRef(ObjectId),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemRecord {
    pub id: ObjectId,
    pub definition_id: ContentDefinitionId,
    pub stack: StackState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub durability: Option<Durability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container: Option<ContainerState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contained_by: Option<ObjectId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owned_by: Option<ActorId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub equipped: Option<EquippedState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub located_at: Option<ObjectId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_name: Option<DisplayName>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bound_actor: Option<ActorId>,
    pub parameters: BTreeMap<ContentDefinitionId, ParameterValue>,
    pub instance_adjustments: Vec<AttributeAdjustment>,
    pub origin: EntityOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConditionSource {
    Item { item_id: ObjectId },
    Skill { grant_id: ObjectId },
    Environment { place_id: ObjectId },
    Character { actor_id: ActorId },
    System { source_id: ContentDefinitionId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntensityPolicy {
    Keep,
    Replace,
    Maximum,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionRecord {
    pub id: ObjectId,
    pub target_id: ActorId,
    pub condition_id: ContentDefinitionId,
    pub source: ConditionSource,
    pub stacks: NonZeroU32,
    pub intensity: Fixed,
    pub applied_at: WorldTime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<WorldTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_periodic_at: Option<WorldTime>,
    pub origin: EntityOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SkillSource {
    CharacterDefinition { definition_id: ContentDefinitionId },
    Item { item_id: ObjectId },
    Condition { condition_id: ObjectId },
    Rule { rule_id: ContentDefinitionId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillGrantRecord {
    pub id: ObjectId,
    pub owner_id: ActorId,
    pub skill_id: ContentDefinitionId,
    pub rank: u32,
    pub proficiency: u32,
    pub source: SkillSource,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ready_at: Option<WorldTime>,
    pub origin: EntityOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationshipRecord {
    pub id: ObjectId,
    pub source_id: ObjectId,
    pub target_id: ObjectId,
    pub kind_id: ContentDefinitionId,
    pub strength: Fixed,
    pub origin: EntityOrigin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum FactSubject {
    World,
    Object { object_id: ObjectId },
    Scene { scene_id: ObjectId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum FactValue {
    Bool(bool),
    Fixed(Fixed),
    Counter(i64),
    BoundedText(ShortText),
    ObjectRef(ObjectId),
    Tag(ContentDefinitionId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeStatus {
    Believed,
    Confirmed,
    Disputed,
    Forgotten,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum FactSource {
    DirectObservation { event_id: EventId },
    Actor { actor_id: ActorId },
    Rule { rule_id: ContentDefinitionId },
    Content { definition_id: ContentDefinitionId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnownFactRecord {
    pub id: ObjectId,
    pub owner_id: ActorId,
    pub subject: FactSubject,
    pub predicate_id: ContentDefinitionId,
    pub value: FactValue,
    pub status: KnowledgeStatus,
    pub confidence: Fixed,
    pub source: FactSource,
    pub first_known_at: WorldTime,
    pub last_confirmed_at: WorldTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    Active,
    Achieved,
    Abandoned,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum GoalSource {
    CharacterDefinition { definition_id: ContentDefinitionId },
    Actor { actor_id: ActorId },
    Rule { rule_id: ContentDefinitionId },
    Event { event_id: EventId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoalRecord {
    pub id: ObjectId,
    pub owner_id: ActorId,
    pub description: ShortText,
    pub priority: i32,
    pub status: GoalStatus,
    pub source: GoalSource,
    pub updated_at: WorldTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventStatus {
    Active,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventInstanceRecord {
    pub id: ObjectId,
    pub definition_id: ContentDefinitionId,
    pub current_node: ContentDefinitionId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene_id: Option<ObjectId>,
    pub started_at: WorldTime,
    pub status: EventStatus,
    pub committed_options: Vec<ContentDefinitionId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterSetRecord {
    pub id: ObjectId,
    pub schema_id: ContentDefinitionId,
    pub values: BTreeMap<ContentDefinitionId, ParameterValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleStateRecord {
    pub id: ObjectId,
    pub definition_id: ContentDefinitionId,
    pub values: BTreeMap<ContentDefinitionId, ParameterValue>,
    pub trigger_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_triggered_at: Option<WorldTime>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum TranscriptSpeaker {
    Player {
        actor_id: ActorId,
        display_name: DisplayName,
    },
    Narrator,
    Actor {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        actor_id: Option<ActorId>,
        display_name: DisplayName,
    },
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptState {
    Committed,
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptItemRecord {
    pub id: TranscriptItemId,
    pub session_id: SessionId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<Revision>,
    pub speaker: TranscriptSpeaker,
    pub text: LongText,
    pub state: TranscriptState,
    pub supporting_events: Vec<EventId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacementInput {
    pub scene_id: ObjectId,
    pub place_id: ObjectId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionGrantInput {
    pub condition_id: ContentDefinitionId,
    pub source: ConditionSource,
    pub stacks: NonZeroU32,
    pub intensity: Fixed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemGrantInput {
    pub local_key: ShortText,
    pub definition_id: ContentDefinitionId,
    pub quantity: NonZeroU32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_local_key: Option<ShortText>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillGrantInput {
    pub skill_id: ContentDefinitionId,
    pub rank: u32,
    pub proficiency: u32,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnownFactInput {
    pub subject: FactSubject,
    pub predicate_id: ContentDefinitionId,
    pub value: FactValue,
    pub status: KnowledgeStatus,
    pub confidence: Fixed,
    pub source: FactSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoalInput {
    pub description: ShortText,
    pub priority: i32,
    pub source: GoalSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpawnConstraints {
    pub minimum_attributes: BTreeMap<ContentDefinitionId, Fixed>,
    pub maximum_attributes: BTreeMap<ContentDefinitionId, Fixed>,
    pub maximum_attribute_points: Fixed,
    pub maximum_items: u32,
    pub maximum_skills: u32,
    pub allowed_definitions: BTreeSet<ContentDefinitionId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CharacterSpawnSpec {
    pub origin: CharacterOrigin,
    pub display_name: DisplayName,
    pub profile: CharacterProfile,
    pub controller: CharacterController,
    pub lifetime: CharacterLifetime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_binding: Option<AgentBinding>,
    pub placement: PlacementInput,
    pub attributes: BaseAttributes,
    pub resources: BTreeMap<ContentDefinitionId, ResourcePool>,
    pub conditions: Vec<ConditionGrantInput>,
    pub inventory: Vec<ItemGrantInput>,
    pub skills: Vec<SkillGrantInput>,
    pub knowledge: Vec<KnownFactInput>,
    pub goals: Vec<GoalInput>,
    pub trusted_constraints: SpawnConstraints,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn domain_payloads_reject_unknown_fields() {
        let raw = json!({
            "id": "wld_01890f6a-2b3c-7d4e-8f90-123456789abc",
            "player_actor": "obj_01890f6a-2b3d-7d4e-8f90-123456789abc",
            "active_scene": "obj_01890f6a-2b3e-7d4e-8f90-123456789abc",
            "clock": 0,
            "rng_seed": [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            "future": true
        });
        assert!(serde_json::from_value::<WorldStateRecord>(raw).is_err());
    }

    #[test]
    fn content_hash_is_canonical_lower_hex() {
        assert!(ContentHash::parse("a".repeat(64)).is_ok());
        assert!(ContentHash::parse("A".repeat(64)).is_err());
        assert!(ContentHash::parse("a".repeat(63)).is_err());
    }
}
