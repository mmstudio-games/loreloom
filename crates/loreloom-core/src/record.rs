use std::{collections::BTreeMap, fmt, num::NonZeroU32, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use serde_json::Value;
use thiserror::Error;

use crate::{ActionId, ContentDefinitionId, ModId, Revision, RevisionError};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RecordError {
    #[error("record type must be bounded lowercase snake_case ASCII")]
    InvalidRecordType,
    #[error("record id must be bounded lowercase ASCII")]
    InvalidRecordId,
    #[error("schema version must be non-zero")]
    InvalidSchemaVersion,
    #[error("record payload must be a JSON object")]
    PayloadMustBeObject,
    #[error("record payload contains a floating-point or out-of-range number")]
    UnsupportedNumber,
    #[error("record type {record_type} is not registered")]
    UnknownRecordType { record_type: RecordType },
    #[error("record {record_type} schema {observed} is newer than supported schema {supported}")]
    NewerSchema {
        record_type: RecordType,
        observed: SchemaVersion,
        supported: SchemaVersion,
    },
    #[error("migration for {record_type} schema {from} is missing")]
    MissingMigration {
        record_type: RecordType,
        from: SchemaVersion,
    },
    #[error("migration registration is duplicate or not a single-version step")]
    InvalidMigration,
    #[error("record op revision is not contiguous after {previous}")]
    RevisionGap { previous: Revision },
    #[error("record op order for revision {revision} is not contiguous at {expected}")]
    InvalidOperationOrder { revision: Revision, expected: u32 },
    #[error("record op action changed within revision {revision}")]
    ActionChanged { revision: Revision },
    #[error("upsert envelope revision does not match its record op")]
    EnvelopeRevisionMismatch,
    #[error("record {key} already exists in a checkpoint")]
    DuplicateCheckpointRecord { key: RecordKey },
    #[error("record {key} cannot be deleted because it does not exist")]
    DeleteMissingRecord { key: RecordKey },
    #[error(transparent)]
    Revision(#[from] RevisionError),
    #[error("record migration failed: {message}")]
    MigrationFailed { message: String },
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecordType(String);

impl RecordType {
    pub const MAX_LEN: usize = 64;

    pub fn parse(value: impl Into<String>) -> Result<Self, RecordError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= Self::MAX_LEN
            && value.is_ascii()
            && value.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
            && value
                .as_bytes()
                .last()
                .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
            && value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
        if !valid || value.contains("__") {
            return Err(RecordError::InvalidRecordType);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RecordType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl fmt::Debug for RecordType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl FromStr for RecordType {
    type Err = RecordError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl Serialize for RecordType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for RecordType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::parse(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecordId(String);

impl RecordId {
    pub const MAX_LEN: usize = 255;

    pub fn parse(value: impl Into<String>) -> Result<Self, RecordError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= Self::MAX_LEN
            && value.is_ascii()
            && value.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'_' | b'-' | b'.' | b':' | b'/')
            });
        if !valid {
            return Err(RecordError::InvalidRecordId);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RecordId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl fmt::Debug for RecordId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl FromStr for RecordId {
    type Err = RecordError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl Serialize for RecordId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for RecordId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::parse(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SchemaVersion(NonZeroU32);

impl SchemaVersion {
    pub const V1: Self = Self(NonZeroU32::MIN);

    pub fn new(value: u32) -> Result<Self, RecordError> {
        NonZeroU32::new(value)
            .map(Self)
            .ok_or(RecordError::InvalidSchemaVersion)
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0.get()
    }

    pub fn next(self) -> Result<Self, RecordError> {
        self.get()
            .checked_add(1)
            .ok_or(RecordError::InvalidSchemaVersion)
            .and_then(Self::new)
    }
}

impl fmt::Display for SchemaVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.get().fmt(formatter)
    }
}

impl Serialize for SchemaVersion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u32(self.get())
    }
}

impl<'de> Deserialize<'de> for SchemaVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(u32::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordProvenance {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_id: Option<ActionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mod_id: Option<ModId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition_id: Option<ContentDefinitionId>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordKey {
    pub record_type: RecordType,
    pub record_id: RecordId,
}

impl fmt::Display for RecordKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.record_type, self.record_id)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RecordEnvelope {
    record_type: RecordType,
    schema_version: SchemaVersion,
    record_id: RecordId,
    revision: Revision,
    payload: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    provenance: Option<RecordProvenance>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordEnvelopeWire {
    record_type: RecordType,
    schema_version: SchemaVersion,
    record_id: RecordId,
    revision: Revision,
    payload: Value,
    #[serde(default)]
    provenance: Option<RecordProvenance>,
}

impl RecordEnvelope {
    pub fn new(
        record_type: RecordType,
        schema_version: SchemaVersion,
        record_id: RecordId,
        revision: Revision,
        payload: Value,
        provenance: Option<RecordProvenance>,
    ) -> Result<Self, RecordError> {
        validate_payload(&payload)?;
        Ok(Self {
            record_type,
            schema_version,
            record_id,
            revision,
            payload,
            provenance,
        })
    }

    #[must_use]
    pub fn key(&self) -> RecordKey {
        RecordKey {
            record_type: self.record_type.clone(),
            record_id: self.record_id.clone(),
        }
    }

    #[must_use]
    pub fn record_type(&self) -> &RecordType {
        &self.record_type
    }

    #[must_use]
    pub const fn schema_version(&self) -> SchemaVersion {
        self.schema_version
    }

    #[must_use]
    pub fn record_id(&self) -> &RecordId {
        &self.record_id
    }

    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    #[must_use]
    pub fn payload(&self) -> &Value {
        &self.payload
    }

    #[must_use]
    pub fn provenance(&self) -> Option<&RecordProvenance> {
        self.provenance.as_ref()
    }
}

impl<'de> Deserialize<'de> for RecordEnvelope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = RecordEnvelopeWire::deserialize(deserializer)?;
        Self::new(
            wire.record_type,
            wire.schema_version,
            wire.record_id,
            wire.revision,
            wire.payload,
            wire.provenance,
        )
        .map_err(de::Error::custom)
    }
}

fn validate_payload(payload: &Value) -> Result<(), RecordError> {
    if !payload.is_object() {
        return Err(RecordError::PayloadMustBeObject);
    }
    validate_json_value(payload)
}

fn validate_json_value(value: &Value) -> Result<(), RecordError> {
    match value {
        Value::Number(number) if !number.is_i64() && !number.is_u64() => {
            Err(RecordError::UnsupportedNumber)
        }
        Value::Array(values) => values.iter().try_for_each(validate_json_value),
        Value::Object(values) => values.values().try_for_each(validate_json_value),
        _ => Ok(()),
    }
}

pub type MigrationFn = fn(Value) -> Result<Value, RecordError>;

#[derive(Clone)]
pub struct MigrationStep {
    pub record_type: RecordType,
    pub from: SchemaVersion,
    pub to: SchemaVersion,
    pub migrate: MigrationFn,
}

#[derive(Default)]
pub struct MigrationRegistry {
    current: BTreeMap<RecordType, SchemaVersion>,
    steps: BTreeMap<(RecordType, SchemaVersion), MigrationStep>,
}

impl MigrationRegistry {
    pub fn register_record_type(
        &mut self,
        record_type: RecordType,
        current: SchemaVersion,
    ) -> Result<(), RecordError> {
        if self.current.insert(record_type, current).is_some() {
            return Err(RecordError::InvalidMigration);
        }
        Ok(())
    }

    pub fn register_migration(&mut self, step: MigrationStep) -> Result<(), RecordError> {
        if step.from.next()? != step.to
            || self
                .steps
                .insert((step.record_type.clone(), step.from), step)
                .is_some()
        {
            return Err(RecordError::InvalidMigration);
        }
        Ok(())
    }

    pub fn upgrade(&self, mut envelope: RecordEnvelope) -> Result<RecordEnvelope, RecordError> {
        let record_type = envelope.record_type.clone();
        let supported = self.current.get(&record_type).copied().ok_or_else(|| {
            RecordError::UnknownRecordType {
                record_type: record_type.clone(),
            }
        })?;
        if envelope.schema_version > supported {
            return Err(RecordError::NewerSchema {
                record_type,
                observed: envelope.schema_version,
                supported,
            });
        }

        while envelope.schema_version < supported {
            let from = envelope.schema_version;
            let step = self
                .steps
                .get(&(record_type.clone(), from))
                .ok_or_else(|| RecordError::MissingMigration {
                    record_type: record_type.clone(),
                    from,
                })?;
            let payload = (step.migrate)(envelope.payload)?;
            validate_payload(&payload)?;
            envelope.payload = payload;
            envelope.schema_version = step.to;
        }
        Ok(envelope)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RecordMutation {
    Upsert { record: RecordEnvelope },
    Delete { key: RecordKey },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionedRecordOp {
    pub revision: Revision,
    pub action_id: ActionId,
    pub order: u32,
    pub mutation: RecordMutation,
}

pub type RecordSet = BTreeMap<RecordKey, RecordEnvelope>;

pub fn rebuild_records(
    checkpoint_revision: Revision,
    checkpoint: impl IntoIterator<Item = RecordEnvelope>,
    operations: impl IntoIterator<Item = VersionedRecordOp>,
) -> Result<(Revision, RecordSet), RecordError> {
    let mut records = RecordSet::new();
    for record in checkpoint {
        if record.revision() > checkpoint_revision {
            return Err(RecordError::EnvelopeRevisionMismatch);
        }
        let key = record.key();
        if records.insert(key.clone(), record).is_some() {
            return Err(RecordError::DuplicateCheckpointRecord { key });
        }
    }

    let mut current_revision = checkpoint_revision;
    let mut current_action = None;
    let mut expected_order = 0_u32;
    for operation in operations {
        if operation.revision != current_revision {
            let expected_revision = current_revision.next()?;
            if operation.revision != expected_revision {
                return Err(RecordError::RevisionGap {
                    previous: current_revision,
                });
            }
            current_revision = operation.revision;
            current_action = Some(operation.action_id);
            expected_order = 0;
        } else if current_action.is_none() {
            return Err(RecordError::RevisionGap {
                previous: current_revision,
            });
        }

        if current_action != Some(operation.action_id) {
            return Err(RecordError::ActionChanged {
                revision: current_revision,
            });
        }
        if operation.order != expected_order {
            return Err(RecordError::InvalidOperationOrder {
                revision: current_revision,
                expected: expected_order,
            });
        }
        expected_order =
            expected_order
                .checked_add(1)
                .ok_or(RecordError::InvalidOperationOrder {
                    revision: current_revision,
                    expected: u32::MAX,
                })?;

        match operation.mutation {
            RecordMutation::Upsert { record } => {
                if record.revision() != current_revision {
                    return Err(RecordError::EnvelopeRevisionMismatch);
                }
                records.insert(record.key(), record);
            }
            RecordMutation::Delete { key } => {
                if records.remove(&key).is_none() {
                    return Err(RecordError::DeleteMissingRecord { key });
                }
            }
        }
    }
    Ok((current_revision, records))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const ACTION_1: &str = "act_01890f6a-2b3c-7d4e-8f90-123456789abc";
    const ACTION_2: &str = "act_01890f6a-2b3d-7d4e-8f90-123456789abc";

    fn envelope(revision: u64, value: i64) -> RecordEnvelope {
        RecordEnvelope::new(
            RecordType::parse("character").expect("record type"),
            SchemaVersion::V1,
            RecordId::parse("obj_01890f6a-2b3c-7d4e-8f90-123456789abc").expect("record id"),
            Revision::new(revision),
            json!({"value": value}),
            None,
        )
        .expect("record envelope")
    }

    #[test]
    fn envelope_rejects_unknown_fields_and_float_payloads() {
        let valid = serde_json::to_value(envelope(0, 1)).expect("serialize envelope");
        let mut unknown = valid.clone();
        unknown
            .as_object_mut()
            .expect("envelope object")
            .insert("future_control".into(), json!(true));
        assert!(serde_json::from_value::<RecordEnvelope>(unknown).is_err());

        let mut float = valid;
        float
            .get_mut("payload")
            .and_then(Value::as_object_mut)
            .expect("payload object")
            .insert("value".into(), json!(1.5));
        assert!(serde_json::from_value::<RecordEnvelope>(float).is_err());
        assert!(
            RecordEnvelope::new(
                RecordType::parse("character").expect("record type"),
                SchemaVersion::V1,
                RecordId::parse("object").expect("record id"),
                Revision::ZERO,
                Value::Null,
                None,
            )
            .is_err()
        );
    }

    #[test]
    fn migration_registry_requires_a_contiguous_chain() {
        fn add_name(mut payload: Value) -> Result<Value, RecordError> {
            let object = payload
                .as_object_mut()
                .ok_or(RecordError::PayloadMustBeObject)?;
            object.insert("name".into(), json!("Mara"));
            Ok(payload)
        }

        let record_type = RecordType::parse("character").expect("record type");
        let mut registry = MigrationRegistry::default();
        registry
            .register_record_type(record_type.clone(), SchemaVersion::new(2).expect("v2"))
            .expect("register record type");
        let original = envelope(0, 1);
        assert!(matches!(
            registry.upgrade(original.clone()),
            Err(RecordError::MissingMigration { .. })
        ));
        registry
            .register_migration(MigrationStep {
                record_type,
                from: SchemaVersion::V1,
                to: SchemaVersion::new(2).expect("v2"),
                migrate: add_name,
            })
            .expect("register migration");
        let upgraded = registry.upgrade(original).expect("upgrade record");
        assert_eq!(upgraded.schema_version().get(), 2);
        assert_eq!(upgraded.payload()["name"], "Mara");
    }

    #[test]
    fn rebuild_uses_contiguous_ordered_record_ops() {
        let key = envelope(0, 0).key();
        let action_1: ActionId = ACTION_1.parse().expect("action 1");
        let action_2: ActionId = ACTION_2.parse().expect("action 2");
        let operations = vec![
            VersionedRecordOp {
                revision: Revision::new(1),
                action_id: action_1,
                order: 0,
                mutation: RecordMutation::Upsert {
                    record: envelope(1, 1),
                },
            },
            VersionedRecordOp {
                revision: Revision::new(2),
                action_id: action_2,
                order: 0,
                mutation: RecordMutation::Delete { key: key.clone() },
            },
        ];
        let (revision, records) =
            rebuild_records(Revision::ZERO, [envelope(0, 0)], operations).expect("rebuild");
        assert_eq!(revision, Revision::new(2));
        assert!(!records.contains_key(&key));
    }

    #[test]
    fn rebuild_rejects_revision_and_order_gaps() {
        let action: ActionId = ACTION_1.parse().expect("action");
        let gap = VersionedRecordOp {
            revision: Revision::new(2),
            action_id: action,
            order: 0,
            mutation: RecordMutation::Upsert {
                record: envelope(2, 2),
            },
        };
        assert!(matches!(
            rebuild_records(Revision::ZERO, [], [gap]),
            Err(RecordError::RevisionGap { .. })
        ));

        let order_gap = VersionedRecordOp {
            revision: Revision::new(1),
            action_id: action,
            order: 1,
            mutation: RecordMutation::Upsert {
                record: envelope(1, 1),
            },
        };
        assert!(matches!(
            rebuild_records(Revision::ZERO, [], [order_gap]),
            Err(RecordError::InvalidOperationOrder { .. })
        ));
    }
}
