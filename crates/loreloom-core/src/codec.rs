use std::collections::{BTreeMap, BTreeSet};

use serde::{Serialize, de::DeserializeOwned};
use thiserror::Error;

use crate::record::MigrationFn;
use crate::{
    CharacterController, CharacterRecord, ConditionRecord, EventInstanceRecord, Fixed, GoalRecord,
    ItemRecord, KnownFactRecord, MigrationRegistry, MigrationStep, ParameterSetRecord, PlaceRecord,
    RecordEnvelope, RecordError, RecordId, RecordKey, RecordProvenance, RecordSet, RecordType,
    RelationshipRecord, Revision, RuleStateRecord, SceneRecord, SchemaVersion, SkillGrantRecord,
    TranscriptItemRecord, TranscriptState, WorldStateRecord,
};

const DOMAIN_RECORD_MIGRATIONS: &[(&str, MigrationFn)] = &[
    ("world_state", identity_v1_to_v2),
    ("scene", identity_v1_to_v2),
    ("place", identity_v1_to_v2),
    ("character", generated_origin_v1_to_v2),
    ("item", generated_origin_v1_to_v2),
    ("condition", generated_origin_v1_to_v2),
    ("skill_grant", generated_origin_v1_to_v2),
    ("relationship", identity_v1_to_v2),
    ("known_fact", identity_v1_to_v2),
    ("goal", identity_v1_to_v2),
    ("event_instance", identity_v1_to_v2),
    ("parameter_set", identity_v1_to_v2),
    ("rule_state", identity_v1_to_v2),
    ("transcript_item", identity_v1_to_v2),
];

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
    pub const SCHEMA_VERSION: SchemaVersion = SchemaVersion::V2;

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
    let (records, _) = migrate_domain_records(records)?;
    records
        .iter()
        .map(|(key, envelope)| Ok((key.clone(), DomainRecord::from_envelope(envelope)?)))
        .collect()
}

pub fn migrate_domain_records(records: &RecordSet) -> Result<(RecordSet, bool), DomainError> {
    let registry = domain_migration_registry()?;
    let mut migrated = false;
    let mut upgraded = RecordSet::new();
    for (key, envelope) in records {
        let current = registry.upgrade(envelope.clone())?;
        if current.key() != *key {
            return Err(DomainError::RecordIdentityMismatch);
        }
        migrated |= current.schema_version() != envelope.schema_version();
        upgraded.insert(key.clone(), current);
    }
    Ok((upgraded, migrated))
}

fn domain_migration_registry() -> Result<MigrationRegistry, DomainError> {
    let mut registry = MigrationRegistry::default();
    for (name, migrate) in DOMAIN_RECORD_MIGRATIONS {
        let record_type = RecordType::parse(*name)?;
        registry.register_record_type(record_type.clone(), DomainRecord::SCHEMA_VERSION)?;
        registry.register_migration(MigrationStep {
            record_type,
            from: SchemaVersion::V1,
            to: SchemaVersion::V2,
            migrate: *migrate,
        })?;
    }
    Ok(registry)
}

fn identity_v1_to_v2(payload: serde_json::Value) -> Result<serde_json::Value, RecordError> {
    Ok(payload)
}

fn generated_origin_v1_to_v2(
    mut payload: serde_json::Value,
) -> Result<serde_json::Value, RecordError> {
    let Some(origin) = payload
        .as_object_mut()
        .and_then(|record| record.get_mut("origin"))
        .and_then(serde_json::Value::as_object_mut)
    else {
        return Ok(payload);
    };
    if origin.get("type").and_then(serde_json::Value::as_str) != Some("generated") {
        return Ok(payload);
    }
    if origin.len() != 2 || !origin.contains_key("origin") {
        return migration_failed("invalid v1 generated EntityOrigin");
    }
    let generated = origin
        .get_mut("origin")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| RecordError::MigrationFailed {
            message: "invalid v1 GeneratedOrigin".to_owned(),
        })?;
    if generated.len() != 3
        || !generated.contains_key("generation_id")
        || !generated.contains_key("generator_version")
        || !generated.contains_key("source_event")
    {
        return migration_failed("invalid v1 GeneratedOrigin fields");
    }
    let event_id =
        generated
            .remove("source_event")
            .ok_or_else(|| RecordError::MigrationFailed {
                message: "missing v1 GeneratedOrigin source_event".to_owned(),
            })?;
    generated.insert(
        "source".to_owned(),
        serde_json::json!({
            "type": "world_event",
            "event_id": event_id,
        }),
    );
    Ok(payload)
}

fn migration_failed<T>(message: &str) -> Result<T, RecordError> {
    Err(RecordError::MigrationFailed {
        message: message.to_owned(),
    })
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

    use crate::{
        ActionState, ActorId, CharacterLifetime, CharacterProfile, DisplayName, EntityOrigin,
        EventId, GeneratedOrigin, GenerationId, GenerationSource, LifeState, ObjectId, Posture,
        RecordProvenance, ShortText, TranscriptItemId, TranscriptSpeaker,
    };

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
        assert_eq!(
            DomainRecord::from_envelope(&envelope).expect("decode"),
            record
        );
    }

    #[test]
    fn generated_origin_v1_migrates_to_world_event_source_before_typed_decode() {
        let event_id = parse::<EventId>("evt_01890f6a-2b3f-7d4e-8f90-123456789abc");
        let record = DomainRecord::Character(CharacterRecord {
            id: parse("obj_01890f6a-2b3d-7d4e-8f90-123456789abc"),
            display_name: DisplayName::new("Mara").expect("display name"),
            profile: CharacterProfile {
                summary: ShortText::new("A generated witness.").expect("summary"),
                values: Vec::new(),
                speaking_style: ShortText::new("Careful.").expect("speaking style"),
                narrative_tags: BTreeSet::new(),
            },
            controller: CharacterController::Agent,
            lifetime: CharacterLifetime::Persistent,
            location: parse("obj_01890f6a-2b3e-7d4e-8f90-123456789abc"),
            inventory_root: parse("obj_01890f6a-2b3f-7d4e-8f90-123456789abc"),
            agent_binding: None,
            base_attributes: crate::BaseAttributes::default(),
            attribute_adjustments: Vec::new(),
            resources: BTreeMap::new(),
            life_state: LifeState::Alive,
            action_state: ActionState::Idle,
            posture: Posture::Standing,
            origin: EntityOrigin::Generated {
                origin: GeneratedOrigin {
                    generation_id: parse::<GenerationId>(
                        "gen_01890f6a-2b40-7d4e-8f90-123456789abc",
                    ),
                    generator_version: ShortText::new("narrator-v1").expect("generator version"),
                    source: GenerationSource::WorldEvent { event_id },
                },
            },
        });
        let current = record
            .to_envelope(
                Revision::new(7),
                Some(RecordProvenance {
                    action_id: Some(parse("act_01890f6a-2b41-7d4e-8f90-123456789abc")),
                    mod_id: None,
                    definition_id: None,
                }),
            )
            .expect("current envelope");
        let mut legacy_payload = current.payload().clone();
        let generated = legacy_payload
            .pointer_mut("/origin/origin")
            .and_then(serde_json::Value::as_object_mut)
            .expect("generated origin object");
        let source = generated
            .remove("source")
            .and_then(|value| value.get("event_id").cloned())
            .expect("world event source");
        generated.insert("source_event".to_owned(), source);
        let legacy = RecordEnvelope::new(
            current.record_type().clone(),
            SchemaVersion::V1,
            current.record_id().clone(),
            current.revision(),
            legacy_payload,
            current.provenance().cloned(),
        )
        .expect("legacy envelope");
        let records = RecordSet::from([(legacy.key(), legacy.clone())]);

        let (upgraded, migrated) = migrate_domain_records(&records).expect("migrate records");
        let upgraded = upgraded.get(&legacy.key()).expect("upgraded character");
        assert!(migrated);
        assert_eq!(upgraded.schema_version(), SchemaVersion::V2);
        assert_eq!(upgraded.record_id(), legacy.record_id());
        assert_eq!(upgraded.revision(), legacy.revision());
        assert_eq!(upgraded.provenance(), legacy.provenance());
        assert_eq!(
            decode_domain_records(&records)
                .expect("decode migrated records")
                .get(&legacy.key()),
            Some(&record)
        );
    }

    #[test]
    fn every_domain_record_has_an_explicit_v1_to_v2_step() {
        let registry = domain_migration_registry().expect("domain migration registry");
        for (name, _) in DOMAIN_RECORD_MIGRATIONS {
            let payload = serde_json::json!({"sentinel": name});
            let envelope = RecordEnvelope::new(
                RecordType::parse(*name).expect("record type"),
                SchemaVersion::V1,
                RecordId::parse("obj_01890f6a-2b42-7d4e-8f90-123456789abc").expect("record id"),
                Revision::ZERO,
                payload.clone(),
                None,
            )
            .expect("legacy envelope");
            let upgraded = registry.upgrade(envelope).expect("identity migration");
            assert_eq!(upgraded.schema_version(), SchemaVersion::V2);
            assert_eq!(upgraded.payload(), &payload);
        }
    }

    #[test]
    fn domain_migration_rejects_unknown_and_newer_record_schemas() {
        let registry = domain_migration_registry().expect("domain migration registry");
        let unknown = RecordEnvelope::new(
            RecordType::parse("future_record").expect("record type"),
            SchemaVersion::V1,
            RecordId::parse("obj_01890f6a-2b43-7d4e-8f90-123456789abc").expect("record id"),
            Revision::ZERO,
            serde_json::json!({}),
            None,
        )
        .expect("unknown envelope");
        assert!(matches!(
            registry.upgrade(unknown),
            Err(RecordError::UnknownRecordType { .. })
        ));

        let newer = RecordEnvelope::new(
            RecordType::parse("character").expect("record type"),
            SchemaVersion::new(3).expect("v3"),
            RecordId::parse("obj_01890f6a-2b44-7d4e-8f90-123456789abc").expect("record id"),
            Revision::ZERO,
            serde_json::json!({}),
            None,
        )
        .expect("newer envelope");
        assert!(matches!(
            registry.upgrade(newer),
            Err(RecordError::NewerSchema { .. })
        ));
    }

    #[test]
    fn generated_origin_migration_rejects_unknown_v1_fields() {
        let envelope = RecordEnvelope::new(
            RecordType::parse("character").expect("record type"),
            SchemaVersion::V1,
            RecordId::parse("obj_01890f6a-2b45-7d4e-8f90-123456789abc").expect("record id"),
            Revision::ZERO,
            serde_json::json!({
                "origin": {
                    "type": "generated",
                    "origin": {
                        "generation_id": "gen_01890f6a-2b46-7d4e-8f90-123456789abc",
                        "generator_version": "narrator-v1",
                        "source_event": "evt_01890f6a-2b47-7d4e-8f90-123456789abc",
                        "future": true
                    }
                }
            }),
            None,
        )
        .expect("legacy envelope");
        assert!(matches!(
            domain_migration_registry()
                .expect("domain migration registry")
                .upgrade(envelope),
            Err(RecordError::MigrationFailed { .. })
        ));
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
