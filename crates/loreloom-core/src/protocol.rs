use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{
    ActionId, ActorId, CharacterSpawnSpec, ContentDefinitionId, DomainError, DomainRecord, EventId,
    Fixed, ObjectId, ParameterValue, RecordKey, RecordMutation, RecordProvenance, Revision,
    ShortText, TranscriptItemRecord, VersionedRecordOp, WorldTime,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldCommand {
    pub action_id: ActionId,
    pub actor_id: ActorId,
    pub expected_revision: Revision,
    pub kind: WorldCommandKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SceneTransitionTarget {
    Existing {
        scene_id: ObjectId,
    },
    Definition {
        scene_definition_id: ContentDefinitionId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorldCommandKind {
    Move {
        destination_id: ObjectId,
    },
    TransferItem {
        item_id: ObjectId,
        container_id: ObjectId,
    },
    EquipItem {
        item_id: ObjectId,
        slot_id: ContentDefinitionId,
    },
    SplitStack {
        item_id: ObjectId,
        quantity: u32,
    },
    UseSkill {
        grant_id: ObjectId,
        target: SkillTargetRef,
    },
    AdvanceTime {
        ticks: u64,
    },
    SpawnCharacter {
        spec: Box<CharacterSpawnSpec>,
    },
    PromoteCharacter {
        actor_id: ActorId,
    },
    TransitionScene {
        target: SceneTransitionTarget,
    },
    AppendTranscript {
        items: Vec<TranscriptItemRecord>,
    },
    ChooseEventOption {
        event_instance_id: ObjectId,
        option_id: ContentDefinitionId,
    },
    PerformGameplayAction {
        action_id: ContentDefinitionId,
        arguments: BTreeMap<ContentDefinitionId, ParameterValue>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SkillTargetRef {
    SelfTarget,
    Object { object_id: ObjectId },
    Place { place_id: ObjectId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldEvent {
    pub id: EventId,
    pub action_id: ActionId,
    pub actor_id: ActorId,
    pub revision: Revision,
    pub kind: WorldEventKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorldEventKind {
    CharacterMoved {
        character_id: ActorId,
        from: ObjectId,
        to: ObjectId,
    },
    ItemTransferred {
        item_id: ObjectId,
        from: ObjectId,
        to: ObjectId,
    },
    ItemEquipped {
        item_id: ObjectId,
        wearer_id: ActorId,
        slot_id: ContentDefinitionId,
    },
    StackSplit {
        source_item_id: ObjectId,
        new_item_id: ObjectId,
        quantity: u32,
    },
    SkillUsed {
        grant_id: ObjectId,
        skill_id: ContentDefinitionId,
        target: SkillTargetRef,
    },
    ClockAdvanced {
        from: u64,
        to: u64,
    },
    CharacterSpawned {
        character_id: ActorId,
    },
    CharacterPromoted {
        character_id: ActorId,
    },
    SceneLeft {
        scene_id: ObjectId,
    },
    SceneEntered {
        scene_id: ObjectId,
    },
    ConditionExpired {
        condition_id: ObjectId,
    },
    ConditionTicked {
        condition_id: ObjectId,
        scheduled_at: WorldTime,
    },
    ResourceChanged {
        character_id: ActorId,
        resource_id: ContentDefinitionId,
        delta: Fixed,
    },
    ConditionApplied {
        character_id: ActorId,
        condition_id: ContentDefinitionId,
        instance_id: ObjectId,
    },
    ItemGranted {
        character_id: ActorId,
        item_id: ObjectId,
        definition_id: ContentDefinitionId,
        quantity: u32,
    },
    SkillGranted {
        character_id: ActorId,
        grant_id: ObjectId,
        skill_id: ContentDefinitionId,
    },
    ParameterChanged {
        parameter_id: ContentDefinitionId,
    },
    GameplayActionPerformed {
        action_id: ContentDefinitionId,
    },
    EventOptionChosen {
        event_instance_id: ObjectId,
        option_id: ContentDefinitionId,
    },
    RuleTriggered {
        rule_id: ContentDefinitionId,
        trigger: ShortText,
    },
    DeclarativeEventEmitted {
        event_type: ShortText,
        source_definition_id: ContentDefinitionId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionChangeSet {
    pub action_id: ActionId,
    pub expected_revision: Revision,
    pub revision: Revision,
    pub upserts: Vec<DomainRecord>,
    pub deletes: Vec<RecordKey>,
    pub events: Vec<WorldEvent>,
    pub safe_summary: ShortText,
}

impl ExecutionChangeSet {
    pub fn record_ops(&self) -> Result<Vec<VersionedRecordOp>, DomainError> {
        let provenance = Some(RecordProvenance {
            action_id: Some(self.action_id),
            mod_id: None,
            definition_id: None,
        });
        let mut mutations = Vec::with_capacity(self.upserts.len() + self.deletes.len());
        let mut seen = BTreeSet::new();
        for record in &self.upserts {
            let key = record.key()?;
            if !seen.insert(key.clone()) {
                return Err(DomainError::InvalidValue {
                    field: "change_set.duplicate_record",
                });
            }
            mutations.push((
                key,
                RecordMutation::Upsert {
                    record: record.to_envelope(self.revision, provenance.clone())?,
                },
            ));
        }
        for key in &self.deletes {
            if !seen.insert(key.clone()) {
                return Err(DomainError::InvalidValue {
                    field: "change_set.duplicate_record",
                });
            }
            mutations.push((key.clone(), RecordMutation::Delete { key: key.clone() }));
        }
        mutations.sort_by(|left, right| left.0.cmp(&right.0));
        mutations
            .into_iter()
            .enumerate()
            .map(|(order, (_, mutation))| {
                let order = u32::try_from(order).map_err(|_| DomainError::InvalidValue {
                    field: "change_set.operation_count",
                })?;
                Ok(VersionedRecordOp {
                    revision: self.revision,
                    action_id: self.action_id,
                    order,
                    mutation,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{WorldId, WorldStateRecord, WorldTime};

    fn parse<T: std::str::FromStr>(value: &str) -> T
    where
        T::Err: std::fmt::Debug,
    {
        value.parse().expect("fixture id")
    }

    #[test]
    fn change_set_orders_record_ops_by_stable_key() {
        let action_id = parse("act_01890f6a-2b3c-7d4e-8f90-123456789abc");
        let state = DomainRecord::WorldState(WorldStateRecord {
            id: parse::<WorldId>("wld_01890f6a-2b3d-7d4e-8f90-123456789abc"),
            player_actor: parse("obj_01890f6a-2b3e-7d4e-8f90-123456789abc"),
            active_scene: parse("obj_01890f6a-2b3f-7d4e-8f90-123456789abc"),
            clock: WorldTime::ZERO,
            rng_seed: [0; 32],
        });
        let changes = ExecutionChangeSet {
            action_id,
            expected_revision: Revision::ZERO,
            revision: Revision::new(1),
            upserts: vec![state],
            deletes: Vec::new(),
            events: Vec::new(),
            safe_summary: ShortText::new("initialized").expect("summary"),
        };
        let operations = changes.record_ops().expect("record operations");
        assert_eq!(operations.len(), 1);
        assert_eq!(operations[0].order, 0);
    }
}
