use std::{
    collections::{BTreeMap, BTreeSet},
    num::{NonZeroU32, NonZeroU64},
};

use loreloom_core::{
    AttributeOperation, AutonomyMode, BaseAttributes, CharacterProfile, ContentDefinitionId,
    DisplayName, FactSubject, FactValue, Fixed, GoalStatus, IntensityPolicy, ParameterValue,
    ShortText, SpawnConstraints,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentDocument {
    pub schema_version: u32,
    pub definitions: Vec<Definition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Definition {
    AgentProfile(AgentProfileDefinition),
    GenerationPolicy(GenerationPolicy),
    Tag(TagDefinition),
    RelationshipKind(RelationshipKindDefinition),
    Attribute(AttributeDefinition),
    Resource(ResourceDefinition),
    EquipmentSlot(EquipmentSlotDefinition),
    Condition(ConditionDefinition),
    Item(ItemDefinition),
    Skill(SkillDefinition),
    Character(CharacterDefinition),
    PlayerCreationForm(PlayerCreationFormDefinition),
    Place(PlaceDefinition),
    Scene(SceneDefinition),
    Parameter(ParameterDefinition),
    Event(EventDefinition),
    GameplayAction(GameplayActionDefinition),
    Rule(RuleDefinition),
}

impl Definition {
    #[must_use]
    pub fn id(&self) -> &ContentDefinitionId {
        match self {
            Self::AgentProfile(value) => &value.id,
            Self::GenerationPolicy(value) => &value.id,
            Self::Tag(value) => &value.id,
            Self::RelationshipKind(value) => &value.id,
            Self::Attribute(value) => &value.id,
            Self::Resource(value) => &value.id,
            Self::EquipmentSlot(value) => &value.id,
            Self::Condition(value) => &value.id,
            Self::Item(value) => &value.id,
            Self::Skill(value) => &value.id,
            Self::Character(value) => &value.id,
            Self::PlayerCreationForm(value) => &value.id,
            Self::Place(value) => &value.id,
            Self::Scene(value) => &value.id,
            Self::Parameter(value) => &value.id,
            Self::Event(value) => &value.id,
            Self::GameplayAction(value) => &value.id,
            Self::Rule(value) => &value.id,
        }
    }

    #[must_use]
    pub const fn expected_kind(&self) -> &'static str {
        match self {
            Self::AgentProfile(_) => "agent_profile",
            Self::GenerationPolicy(_) => "generation_policy",
            Self::Tag(_) => "tag",
            Self::RelationshipKind(_) => "relationship_kind",
            Self::Attribute(_) => "attribute",
            Self::Resource(_) => "resource",
            Self::EquipmentSlot(_) => "equipment_slot",
            Self::Condition(_) => "condition",
            Self::Item(_) => "item",
            Self::Skill(_) => "skill",
            Self::Character(_) => "character",
            Self::PlayerCreationForm(_) => "player_creation_form",
            Self::Place(_) => "place",
            Self::Scene(_) => "scene",
            Self::Parameter(_) => "parameter",
            Self::Event(_) => "event",
            Self::GameplayAction(_) => "gameplay_action",
            Self::Rule(_) => "rule",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TagDefinition {
    pub id: ContentDefinitionId,
    pub display_name: DisplayName,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationshipKindDefinition {
    pub id: ContentDefinitionId,
    pub display_name: DisplayName,
    pub directional: bool,
    pub minimum: Fixed,
    pub maximum: Fixed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentProfileDefinition {
    pub id: ContentDefinitionId,
    pub display_name: DisplayName,
    pub system_style: ShortText,
    pub model_alias: ShortText,
    pub tool_capabilities: BTreeSet<ShortText>,
    pub autonomy: AutonomyMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttributeDefinition {
    pub id: ContentDefinitionId,
    pub display_name: DisplayName,
    pub minimum: Fixed,
    pub maximum: Fixed,
    pub allowed_operations: BTreeSet<AttributeOperation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceMaximumPolicy {
    ClampCurrent,
    PreserveRatio,
    AllowOvercap,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceDefinition {
    pub id: ContentDefinitionId,
    pub display_name: DisplayName,
    pub minimum: Fixed,
    pub maximum: Fixed,
    pub maximum_policy: ResourceMaximumPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derived_from_attribute: Option<ContentDefinitionId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EquipmentSlotDefinition {
    pub id: ContentDefinitionId,
    pub display_name: DisplayName,
    pub allowed_item_tags: BTreeSet<ContentDefinitionId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModifierDefinition {
    pub attribute_id: ContentDefinitionId,
    pub operation: AttributeOperation,
    pub value: Fixed,
    pub priority: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum StackPolicy {
    Unique,
    RefreshDuration,
    IncreaseStacks {
        maximum: NonZeroU32,
        refresh_duration: bool,
    },
    IndependentInstances {
        maximum: NonZeroU32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum DurationPolicy {
    Permanent,
    Finite { ticks: NonZeroU64 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SymptomDefinition {
    pub text: ShortText,
    pub minimum_intensity: Fixed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeriodicEffectDefinition {
    pub interval_ticks: NonZeroU64,
    pub effects: Vec<EffectDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionDefinition {
    pub id: ContentDefinitionId,
    pub display_name: DisplayName,
    pub tags: BTreeSet<ContentDefinitionId>,
    pub stack_policy: StackPolicy,
    pub intensity_policy: IntensityPolicy,
    pub duration: DurationPolicy,
    pub symptoms: Vec<SymptomDefinition>,
    pub modifiers: Vec<ModifierDefinition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub periodic: Option<PeriodicEffectDefinition>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DurabilityDefinition {
    pub maximum: Fixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContainerDefinition {
    pub max_weight_grams: Fixed,
    pub max_children: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemDefinition {
    pub id: ContentDefinitionId,
    pub display_name: DisplayName,
    pub description: ShortText,
    pub tags: BTreeSet<ContentDefinitionId>,
    pub stack_limit: NonZeroU32,
    pub unit_weight_grams: Fixed,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub durability: Option<DurabilityDefinition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container: Option<ContainerDefinition>,
    pub equipment_slots: BTreeSet<ContentDefinitionId>,
    pub modifiers: Vec<ModifierDefinition>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillKind {
    Active,
    Passive,
    Reaction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceCost {
    pub resource_id: ContentDefinitionId,
    pub amount: Fixed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SkillTarget {
    SelfTarget,
    Character {
        allow_self: bool,
        maximum_range: Fixed,
    },
    Object {
        allowed_kinds: BTreeSet<ShortText>,
        maximum_range: Fixed,
    },
    Place {
        maximum_range: Fixed,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReactionWindow {
    pub event_type: ShortText,
    pub predicates: Vec<PredicateDefinition>,
    pub maximum_triggers_per_action: NonZeroU32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillDefinition {
    pub id: ContentDefinitionId,
    pub display_name: DisplayName,
    pub description: ShortText,
    pub kind: SkillKind,
    pub costs: Vec<ResourceCost>,
    pub cooldown_ticks: u64,
    pub target: SkillTarget,
    pub executor_id: ContentDefinitionId,
    pub effects: Vec<EffectDefinition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reaction: Option<ReactionWindow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PredicateDefinition {
    ResourceAtLeast {
        resource_id: ContentDefinitionId,
        amount: Fixed,
    },
    HasCondition {
        condition_id: ContentDefinitionId,
    },
    HasTag {
        tag_id: ContentDefinitionId,
    },
    Not {
        predicate: Box<PredicateDefinition>,
    },
    All {
        predicates: Vec<PredicateDefinition>,
    },
    Any {
        predicates: Vec<PredicateDefinition>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum EffectDefinition {
    ResourceDelta {
        resource_id: ContentDefinitionId,
        amount: Fixed,
    },
    ApplyCondition {
        condition_id: ContentDefinitionId,
        stacks: NonZeroU32,
        intensity: Fixed,
    },
    GrantItem {
        item_id: ContentDefinitionId,
        quantity: NonZeroU32,
    },
    GrantSkill {
        skill_id: ContentDefinitionId,
        rank: NonZeroU32,
    },
    SetParameter {
        parameter_id: ContentDefinitionId,
        value: ParameterValue,
    },
    EmitEvent {
        event_type: ShortText,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ParameterType {
    Bool,
    Fixed {
        minimum: Fixed,
        maximum: Fixed,
    },
    Counter {
        minimum: i64,
        maximum: i64,
    },
    Enum {
        variants: BTreeSet<ContentDefinitionId>,
    },
    TagSet {
        allowed: BTreeSet<ContentDefinitionId>,
        maximum: u32,
    },
    ObjectRef {
        allowed_kinds: BTreeSet<ShortText>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParameterVisibility {
    Public,
    Narrator,
    Owner,
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParameterPersistence {
    Save,
    Session,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterDefinition {
    pub id: ContentDefinitionId,
    pub display_name: DisplayName,
    pub value_type: ParameterType,
    pub default: ParameterValue,
    pub visibility: ParameterVisibility,
    pub persistence: ParameterPersistence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventOptionDefinition {
    pub id: ContentDefinitionId,
    pub display_name: DisplayName,
    pub visible_if: Vec<PredicateDefinition>,
    pub enabled_if: Vec<PredicateDefinition>,
    pub effects: Vec<EffectDefinition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_node: Option<ContentDefinitionId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventNodeDefinition {
    pub id: ContentDefinitionId,
    pub text: ShortText,
    pub options: Vec<EventOptionDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventDefinition {
    pub id: ContentDefinitionId,
    pub display_name: DisplayName,
    pub entry_node: ContentDefinitionId,
    pub nodes: Vec<EventNodeDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionParameterDefinition {
    pub id: ContentDefinitionId,
    pub value_type: ParameterType,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameplayActionDefinition {
    pub id: ContentDefinitionId,
    pub display_name: DisplayName,
    pub capability: ShortText,
    pub parameters: Vec<ActionParameterDefinition>,
    pub predicates: Vec<PredicateDefinition>,
    pub effects: Vec<EffectDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum TriggerDefinition {
    WorldEvent { event_type: ShortText },
    WorldClock { every_ticks: NonZeroU64 },
    SceneEntered { scene_id: ContentDefinitionId },
    SceneLeft { scene_id: ContentDefinitionId },
    GameplayAction { action_id: ContentDefinitionId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleDefinition {
    pub id: ContentDefinitionId,
    pub priority: i32,
    pub trigger: TriggerDefinition,
    pub predicates: Vec<PredicateDefinition>,
    pub effects: Vec<EffectDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InitialResource {
    pub resource_id: ContentDefinitionId,
    pub current: Fixed,
    pub base_maximum: Fixed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InitialCondition {
    pub condition_id: ContentDefinitionId,
    pub stacks: NonZeroU32,
    pub intensity: Fixed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InitialItem {
    pub local_key: ShortText,
    pub item_id: ContentDefinitionId,
    pub quantity: NonZeroU32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_local_key: Option<ShortText>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InitialSkill {
    pub skill_id: ContentDefinitionId,
    pub rank: NonZeroU32,
    pub proficiency: u32,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InitialFact {
    pub subject: FactSubject,
    pub predicate_id: ContentDefinitionId,
    pub value: FactValue,
    pub confidence: Fixed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InitialGoal {
    pub description: ShortText,
    pub priority: i32,
    pub status: GoalStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CharacterDefinition {
    pub id: ContentDefinitionId,
    pub display_name: DisplayName,
    pub profile: CharacterProfile,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_profile: Option<ContentDefinitionId>,
    pub base_attributes: BaseAttributes,
    pub resources: Vec<InitialResource>,
    pub conditions: Vec<InitialCondition>,
    pub inventory: Vec<InitialItem>,
    pub skills: Vec<InitialSkill>,
    pub knowledge: Vec<InitialFact>,
    pub goals: Vec<InitialGoal>,
    pub spawn_constraints: SpawnConstraints,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerCreationFormDefinition {
    pub id: ContentDefinitionId,
    pub display_name: DisplayName,
    pub description: ShortText,
    pub template: ContentDefinitionId,
    pub fields: Vec<PlayerCreationFieldDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerCreationFieldDefinition {
    pub id: ContentDefinitionId,
    pub display_name: DisplayName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<ShortText>,
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding: Option<PlayerCreationBinding>,
    pub value_type: PlayerCreationFieldType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlayerCreationFieldType {
    Text {
        minimum_bytes: u32,
        maximum_bytes: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<ShortText>,
    },
    LongText {
        minimum_bytes: u32,
        maximum_bytes: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<loreloom_core::LongText>,
    },
    Integer {
        minimum: i64,
        maximum: i64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<i64>,
    },
    Number {
        minimum: Fixed,
        maximum: Fixed,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<Fixed>,
    },
    Boolean {
        default: bool,
    },
    SingleChoice {
        options: Vec<PlayerCreationChoice>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<ContentDefinitionId>,
    },
    MultiChoice {
        minimum_selections: u32,
        maximum_selections: u32,
        options: Vec<PlayerCreationChoice>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<BTreeSet<ContentDefinitionId>>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlayerCreationBinding {
    DisplayName,
    ProfileSummary,
    ProfileSpeakingStyle,
    ProfileValue,
    Attribute { attribute_id: ContentDefinitionId },
    Parameter { parameter_id: ContentDefinitionId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerCreationChoice {
    pub value: ContentDefinitionId,
    pub display_name: DisplayName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<ShortText>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<PlayerCreationEffect>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlayerCreationEffect {
    GrantItem {
        item_id: ContentDefinitionId,
        quantity: NonZeroU32,
    },
    GrantSkill {
        skill_id: ContentDefinitionId,
        rank: NonZeroU32,
        proficiency: u32,
    },
    ApplyCondition {
        condition_id: ContentDefinitionId,
        stacks: NonZeroU32,
        intensity: Fixed,
    },
    SetAttribute {
        attribute_id: ContentDefinitionId,
        value: Fixed,
    },
    SetParameter {
        parameter_id: ContentDefinitionId,
        value: ParameterValue,
    },
    AddNarrativeTag {
        tag_id: ContentDefinitionId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum PlayerCreationFieldValue {
    Text(loreloom_core::LongText),
    Integer(i64),
    Number(Fixed),
    Boolean(bool),
    SingleChoice(ContentDefinitionId),
    MultiChoice(BTreeSet<ContentDefinitionId>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerCreationDraft {
    pub form_id: ContentDefinitionId,
    pub values: BTreeMap<ContentDefinitionId, PlayerCreationFieldValue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayerBootstrap {
    Fixed,
    Preset { character_id: ContentDefinitionId },
    Ugc { draft: PlayerCreationDraft },
}

/// Untrusted model output for a generated NPC. Agent binding, capability, and constraints always
/// come from the separately trusted [`GenerationPolicy`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NpcDraft {
    pub display_name: DisplayName,
    pub profile: CharacterProfile,
    pub base_attributes: BaseAttributes,
    pub resources: Vec<InitialResource>,
    pub conditions: Vec<InitialCondition>,
    pub inventory: Vec<InitialItem>,
    pub skills: Vec<InitialSkill>,
    pub knowledge: Vec<InitialFact>,
    pub goals: Vec<InitialGoal>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationPolicy {
    pub id: ContentDefinitionId,
    pub constraints: SpawnConstraints,
    pub allowed_agent_profiles: BTreeSet<ContentDefinitionId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaceDefinition {
    pub id: ContentDefinitionId,
    pub display_name: DisplayName,
    pub description: ShortText,
    pub tags: BTreeSet<ContentDefinitionId>,
    pub edges: BTreeSet<ContentDefinitionId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneCharacterDefinition {
    pub local_key: ShortText,
    pub character_id: ContentDefinitionId,
    pub place_id: ContentDefinitionId,
    pub controller: InitialCharacterController,
    pub lifetime: InitialCharacterLifetime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InitialCharacterController {
    Player,
    Narrator,
    Rules,
    Agent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InitialCharacterLifetime {
    Scene,
    Persistent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneDefinition {
    pub id: ContentDefinitionId,
    pub display_name: DisplayName,
    pub framing: ShortText,
    pub entry_place: ContentDefinitionId,
    pub places: BTreeSet<ContentDefinitionId>,
    pub characters: Vec<SceneCharacterDefinition>,
}
