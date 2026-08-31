use std::collections::{BTreeMap, BTreeSet};

use serde::{Serialize, de::DeserializeOwned};
use thiserror::Error;

use crate::{
    CharacterController, CharacterRecord, ConditionRecord, EventInstanceRecord, Fixed, GoalRecord,
    ItemRecord, KnownFactRecord, ParameterSetRecord, PlaceRecord, RecordEnvelope, RecordError,
    RecordId, RecordKey, RecordProvenance, RecordSet, RecordType, RelationshipRecord, Revision,
    RuleStateRecord, SceneRecord, SchemaVersion, SkillGrantRecord, TranscriptItemRecord,
    TranscriptState, WorldStateRecord,
};

#[derive(Debug, Error)]
pub enum DomainError {
    #[error(transparent)]
    Record(#[from] RecordError),
    #[error("domain payload codec failed for {record_type}: {message}")]
    Codec {
        record_type: String,
        message: String,
    },
    #[error("record envelope id does not match its typed payload id")]
    RecordIdentityMismatch,
    #[error("invalid domain value: {field}")]
    InvalidValue { field: &'static str },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainRecord {
    WorldState(WorldStateRecord),
    Scene(SceneRecord),
    Place(PlaceRecord),
    Character(CharacterRecord),
    Item(ItemRecord),
    Condition(ConditionRecord),
    SkillGrant(SkillGrantRecord),
    Relationship(RelationshipRecord),
    KnownFact(KnownFactRecord),
    Goal(GoalRecord),
    EventInstance(EventInstanceRecord),
    ParameterSet(ParameterSetRecord),
    RuleState(RuleStateRecord),
    TranscriptItem(TranscriptItemRecord),
}

impl DomainRecord {
    pub const SCHEMA_VERSION: SchemaVersion = SchemaVersion::V1;

    #[must_use]
    pub const fn record_type_name(&self) -> &'static str {
        match self {
            Self::WorldState(_) => "world_state",
            Self::Scene(_) => "scene",
            Self::Place(_) => "place",
            Self::Character(_) => "character",
            Self::Item(_) => "item",
            Self::Condition(_) => "condition",
            Self::SkillGrant(_) => "skill_grant",
            Self::Relationship(_) => "relationship",
            Self::KnownFact(_) => "known_fact",
            Self::Goal(_) => "goal",
            Self::EventInstance(_) => "event_instance",
            Self::ParameterSet(_) => "parameter_set",
            Self::RuleState(_) => "rule_state",
            Self::TranscriptItem(_) => "transcript_item",
        }
    }

    pub fn record_type(&self) -> Result<RecordType, DomainError> {
        Ok(RecordType::parse(self.record_type_name())?)
    }

    pub fn record_id(&self) -> Result<RecordId, DomainError> {
        let value = match self {
            Self::WorldState(value) => value.id.to_string(),
            Self::Scene(value) => value.id.to_string(),
            Self::Place(value) => value.id.to_string(),
            Self::Character(value) => value.id.to_string(),
            Self::Item(value) => value.id.to_string(),
            Self::Condition(value) => value.id.to_string(),
            Self::SkillGrant(value) => value.id.to_string(),
            Self::Relationship(value) => value.id.to_string(),
            Self::KnownFact(value) => value.id.to_string(),
            Self::Goal(value) => value.id.to_string(),
            Self::EventInstance(value) => value.id.to_string(),
            Self::ParameterSet(value) => value.id.to_string(),
            Self::RuleState(value) => value.id.to_string(),
            Self::TranscriptItem(value) => value.id.to_string(),
        };
        Ok(RecordId::parse(value)?)
    }

    pub fn key(&self) -> Result<RecordKey, DomainError> {
        Ok(RecordKey {
            record_type: self.record_type()?,
            record_id: self.record_id()?,
        })
    }

    pub fn to_envelope(
        &self,
        revision: Revision,
        provenance: Option<RecordProvenance>,
    ) -> Result<RecordEnvelope, DomainError> {
        self.validate()?;
        let payload = match self {
            Self::WorldState(value) => encode(value, self.record_type_name())?,
            Self::Scene(value) => encode(value, self.record_type_name())?,
            Self::Place(value) => encode(value, self.record_type_name())?,
            Self::Character(value) => encode(value, self.record_type_name())?,
            Self::Item(value) => encode(value, self.record_type_name())?,
            Self::Condition(value) => encode(value, self.record_type_name())?,
            Self::SkillGrant(value) => encode(value, self.record_type_name())?,
            Self::Relationship(value) => encode(value, self.record_type_name())?,
            Self::KnownFact(value) => encode(value, self.record_type_name())?,
            Self::Goal(value) => encode(value, self.record_type_name())?,
            Self::EventInstance(value) => encode(value, self.record_type_name())?,
            Self::ParameterSet(value) => encode(value, self.record_type_name())?,
            Self::RuleState(value) => encode(value, self.record_type_name())?,
            Self::TranscriptItem(value) => encode(value, self.record_type_name())?,
        };
        Ok(RecordEnvelope::new(
            self.record_type()?,
            Self::SCHEMA_VERSION,
            self.record_id()?,
            revision,
            payload,
            provenance,
        )?)
    }

    pub fn from_envelope(envelope: &RecordEnvelope) -> Result<Self, DomainError> {
        if envelope.schema_version() != Self::SCHEMA_VERSION {
            return Err(DomainError::Codec {
                record_type: envelope.record_type().to_string(),
                message: format!(
                    "expected schema {}, observed {}",
                    Self::SCHEMA_VERSION,
                    envelope.schema_version()
                ),
            });
        }
        let payload = envelope.payload().clone();
        let record_type = envelope.record_type().as_str();
        let record = match record_type {
            "world_state" => Self::WorldState(decode(payload, record_type)?),
            "scene" => Self::Scene(decode(payload, record_type)?),
            "place" => Self::Place(decode(payload, record_type)?),
            "character" => Self::Character(decode(payload, record_type)?),
            "item" => Self::Item(decode(payload, record_type)?),
            "condition" => Self::Condition(decode(payload, record_type)?),
            "skill_grant" => Self::SkillGrant(decode(payload, record_type)?),
            "relationship" => Self::Relationship(decode(payload, record_type)?),
            "known_fact" => Self::KnownFact(decode(payload, record_type)?),
            "goal" => Self::Goal(decode(payload, record_type)?),
            "event_instance" => Self::EventInstance(decode(payload, record_type)?),
            "parameter_set" => Self::ParameterSet(decode(payload, record_type)?),
            "rule_state" => Self::RuleState(decode(payload, record_type)?),
            "transcript_item" => Self::TranscriptItem(decode(payload, record_type)?),
            _ => {
                return Err(DomainError::Record(RecordError::UnknownRecordType {
                    record_type: envelope.record_type().clone(),
                }));
            }
        };
        if record.record_id()? != *envelope.record_id() {
            return Err(DomainError::RecordIdentityMismatch);
        }
        record.validate()?;
        Ok(record)
    }

    pub fn validate(&self) -> Result<(), DomainError> {
        match self {
            Self::WorldState(_) | Self::Scene(_) | Self::Place(_) => Ok(()),
            Self::Character(value) => validate_character(value),
            Self::Item(value) => validate_item(value),
            Self::Condition(value) => validate_condition(value),
            Self::SkillGrant(value) => {
                if value.rank == 0 {
                    invalid("skill_grant.rank")
                } else {
                    Ok(())
                }
            }
            Self::Relationship(value) => {
                if value.source_id == value.target_id {
                    invalid("relationship.self_reference")
                } else {
                    Ok(())
                }
            }
            Self::KnownFact(value) => validate_known_fact(value),
            Self::Goal(_) | Self::EventInstance(_) | Self::ParameterSet(_) | Self::RuleState(_) => {
                Ok(())
            }
            Self::TranscriptItem(value) => validate_transcript(value),
        }
    }
}

pub fn decode_domain_records(
    records: &RecordSet,
) -> Result<BTreeMap<RecordKey, DomainRecord>, DomainError> {
    records
        .iter()
        .map(|(key, envelope)| Ok((key.clone(), DomainRecord::from_envelope(envelope)?)))
        .collect()
}

fn encode<T: Serialize>(value: &T, record_type: &str) -> Result<serde_json::Value, DomainError> {
    serde_json::to_value(value).map_err(|error| DomainError::Codec {
        record_type: record_type.to_owned(),
        message: error.to_string(),
    })
}

fn decode<T: DeserializeOwned>(
    value: serde_json::Value,
    record_type: &str,
) -> Result<T, DomainError> {
    serde_json::from_value(value).map_err(|error| DomainError::Codec {
        record_type: record_type.to_owned(),
        message: error.to_string(),
    })
}

fn invalid(field: &'static str) -> Result<(), DomainError> {
    Err(DomainError::InvalidValue { field })
}

fn validate_character(value: &CharacterRecord) -> Result<(), DomainError> {
    if value.controller != CharacterController::Agent && value.agent_binding.is_some() {
        return invalid("character.agent_binding");
    }
    for (resource_id, pool) in &value.resources {
        if resource_id != &pool.resource_id {
            return invalid("character.resources.key");
        }
        if pool.current < Fixed::ZERO || pool.base_maximum <= Fixed::ZERO {
            return invalid("character.resources.range");
        }
    }
    let mut order = None;
    for adjustment in &value.attribute_adjustments {
        let key = (
            adjustment.operation as u8,
            adjustment.priority,
            adjustment.source_id,
            adjustment.attribute_id.clone(),
        );
        if order.as_ref().is_some_and(|previous| previous > &key) {
            return invalid("character.attribute_adjustments.order");
        }
        order = Some(key);
    }
    Ok(())
}

fn validate_item(value: &ItemRecord) -> Result<(), DomainError> {
    if value.contained_by.is_some() == value.located_at.is_some() {
        return invalid("item.physical_location");
    }
    if let Some(durability) = value.durability
        && (durability.maximum <= Fixed::ZERO
            || durability.current < Fixed::ZERO
            || durability.current > durability.maximum)
    {
        return invalid("item.durability");
    }
    if let Some(container) = value.container
        && (container.max_weight_grams < Fixed::ZERO || container.max_children == 0)
    {
        return invalid("item.container");
    }
    if (value.container.is_some() || value.equipped.is_some()) && value.stack.0.get() != 1 {
        return invalid("item.stackability");
    }
    Ok(())
}

fn validate_condition(value: &ConditionRecord) -> Result<(), DomainError> {
    if value
        .expires_at
        .is_some_and(|expires| expires <= value.applied_at)
        || value
            .next_periodic_at
            .is_some_and(|next| next <= value.applied_at)
    {
        invalid("condition.time")
    } else {
        Ok(())
    }
}

fn validate_known_fact(value: &KnownFactRecord) -> Result<(), DomainError> {
    if value.confidence < Fixed::ZERO
        || value.confidence > Fixed::ONE
        || value.last_confirmed_at < value.first_known_at
    {
        invalid("known_fact.confidence_or_time")
    } else {
        Ok(())
    }
}

fn validate_transcript(value: &TranscriptItemRecord) -> Result<(), DomainError> {
    if value.state == TranscriptState::Committed && value.revision.is_none() {
        return invalid("transcript_item.revision");
    }
    let unique = value
        .supporting_events
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    if unique.len() != value.supporting_events.len() {
        invalid("transcript_item.supporting_events")
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use crate::{ActorId, ObjectId, TranscriptItemId, TranscriptSpeaker};

    use super::*;

    fn parse<T: std::str::FromStr>(value: &str) -> T
    where
        T::Err: std::fmt::Debug,
    {
        value.parse().expect("fixture id")
    }

    #[test]
    fn typed_record_round_trips_through_strict_envelope() {
        let record = DomainRecord::WorldState(WorldStateRecord {
            id: parse("wld_01890f6a-2b3c-7d4e-8f90-123456789abc"),
            player_actor: parse::<ActorId>("obj_01890f6a-2b3d-7d4e-8f90-123456789abc"),
            active_scene: parse("obj_01890f6a-2b3e-7d4e-8f90-123456789abc"),
            clock: crate::WorldTime::ZERO,
            rng_seed: [7; 32],
        });
        let envelope = record
            .to_envelope(Revision::ZERO, None)
            .expect("encode domain record");
        assert_eq!(envelope.schema_version(), SchemaVersion::V1);
        assert_eq!(
            DomainRecord::from_envelope(&envelope).expect("decode"),
            record
        );
    }

    #[test]
    fn transcript_requires_revision_when_committed() {
        let record = DomainRecord::TranscriptItem(TranscriptItemRecord {
            id: parse::<TranscriptItemId>("trn_01890f6a-2b3c-7d4e-8f90-123456789abc"),
            session_id: parse("ses_01890f6a-2b3d-7d4e-8f90-123456789abc"),
            revision: None,
            speaker: TranscriptSpeaker::Actor {
                actor_id: Some(parse::<ActorId>("obj_01890f6a-2b3e-7d4e-8f90-123456789abc")),
                display_name: crate::DisplayName::new("Mara").expect("display name"),
            },
            text: crate::LongText::new("Hello").expect("text"),
            state: TranscriptState::Committed,
            supporting_events: Vec::new(),
        });
        assert!(record.validate().is_err());
    }

    #[test]
    fn item_has_one_physical_location() {
        let item = ItemRecord {
            id: parse::<ObjectId>("obj_01890f6a-2b3c-7d4e-8f90-123456789abc"),
            definition_id: parse("games.loreloom.core:item/coin"),
            stack: crate::StackState(NonZeroU32::new(1).expect("non-zero")),
            durability: None,
            container: None,
            contained_by: None,
            owned_by: None,
            equipped: None,
            located_at: None,
            custom_name: None,
            bound_actor: None,
            parameters: BTreeMap::new(),
            instance_adjustments: Vec::new(),
            origin: crate::EntityOrigin::System {
                source: parse("games.loreloom.core:system/bootstrap"),
            },
        };
        assert!(DomainRecord::Item(item).validate().is_err());
    }
}
