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
