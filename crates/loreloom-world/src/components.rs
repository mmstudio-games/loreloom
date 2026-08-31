use bevy_ecs::prelude::Component;
use loreloom_core::{
    CharacterRecord, ConditionRecord, EventInstanceRecord, GoalRecord, ItemRecord, KnownFactRecord,
    ObjectId, ParameterSetRecord, PlaceRecord, RelationshipRecord, RuleStateRecord, SceneRecord,
    SkillGrantRecord,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Component)]
pub struct PersistentId(pub ObjectId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Component)]
pub enum ObjectKind {
    Scene,
    Place,
    Character,
    Item,
    Condition,
    SkillGrant,
    Relationship,
    KnownFact,
    Goal,
    EventInstance,
    ParameterSet,
    RuleState,
}

impl ObjectKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Scene => "scene",
            Self::Place => "place",
            Self::Character => "character",
            Self::Item => "item",
            Self::Condition => "condition",
            Self::SkillGrant => "skill_grant",
            Self::Relationship => "relationship",
            Self::KnownFact => "known_fact",
            Self::Goal => "goal",
            Self::EventInstance => "event_instance",
            Self::ParameterSet => "parameter_set",
            Self::RuleState => "rule_state",
        }
    }
}

macro_rules! record_component {
    ($name:ident, $record:ty) => {
        #[derive(Debug, Clone, PartialEq, Eq, Component)]
        pub(crate) struct $name(pub $record);
    };
}

record_component!(SceneComponent, SceneRecord);
record_component!(PlaceComponent, PlaceRecord);
record_component!(CharacterComponent, CharacterRecord);
record_component!(ItemComponent, ItemRecord);
record_component!(ConditionComponent, ConditionRecord);
record_component!(SkillGrantComponent, SkillGrantRecord);
record_component!(RelationshipComponent, RelationshipRecord);
record_component!(KnownFactComponent, KnownFactRecord);
record_component!(GoalComponent, GoalRecord);
record_component!(EventInstanceComponent, EventInstanceRecord);
record_component!(ParameterSetComponent, ParameterSetRecord);
record_component!(RuleStateComponent, RuleStateRecord);
