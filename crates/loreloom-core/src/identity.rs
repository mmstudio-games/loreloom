use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;
use uuid::{Uuid, Variant};

const UUID_TEXT_LEN: usize = 36;
const PREFIXED_ID_LEN: usize = 40;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum IdentityError {
    #[error("expected a canonical {expected_prefix}_ UUIDv7 identifier")]
    InvalidRuntimeId { expected_prefix: &'static str },
    #[error("identifier generator returned a UUID that is not RFC 9562 version 7")]
    InvalidGeneratedUuid,
    #[error("mod identifier must be a bounded lowercase reverse-DNS name")]
    InvalidModId,
    #[error("content definition identifier must use mod-id:kind/local-key")]
    InvalidContentDefinitionId,
}

/// Supplies UUIDv7 values to stable ID constructors.
///
/// Production code normally uses [`SystemIdGenerator`]. Tests can inject fixed UUIDv7 values
/// without changing global generator state.
pub trait IdGenerator {
    fn next_uuid_v7(&mut self) -> Uuid;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemIdGenerator;

impl IdGenerator for SystemIdGenerator {
    fn next_uuid_v7(&mut self) -> Uuid {
        Uuid::now_v7()
    }
}

fn validate_uuid_v7(uuid: Uuid) -> Result<Uuid, IdentityError> {
    if uuid.get_variant() == Variant::RFC4122 && uuid.get_version_num() == 7 {
        Ok(uuid)
    } else {
        Err(IdentityError::InvalidGeneratedUuid)
    }
}

fn parse_prefixed(value: &str, prefix: &'static str) -> Result<Uuid, IdentityError> {
    let error = || IdentityError::InvalidRuntimeId {
        expected_prefix: prefix,
    };
    if value.len() != PREFIXED_ID_LEN || !value.starts_with(prefix) || value.as_bytes()[3] != b'_' {
        return Err(error());
    }

    let uuid_text = &value[4..];
    if uuid_text.len() != UUID_TEXT_LEN || uuid_text.bytes().any(|byte| byte.is_ascii_uppercase()) {
        return Err(error());
    }
    let uuid = Uuid::parse_str(uuid_text).map_err(|_| error())?;
    if uuid.hyphenated().to_string() != uuid_text
        || uuid.get_variant() != Variant::RFC4122
        || uuid.get_version_num() != 7
    {
        return Err(error());
    }
    Ok(uuid)
}

macro_rules! runtime_id {
    ($name:ident, $prefix:literal) => {
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(Uuid);

        impl $name {
            pub const PREFIX: &'static str = $prefix;

            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            pub fn generate_with(generator: &mut impl IdGenerator) -> Result<Self, IdentityError> {
                validate_uuid_v7(generator.next_uuid_v7()).map(Self)
            }

            pub fn from_uuid(uuid: Uuid) -> Result<Self, IdentityError> {
                validate_uuid_v7(uuid).map(Self)
            }

            #[must_use]
            pub const fn as_uuid(&self) -> &Uuid {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl FromStr for $name {
            type Err = IdentityError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                parse_prefixed(value, $prefix).map(Self)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, "{}_{:}", $prefix, self.0.hyphenated())
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(self, formatter)
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.collect_str(self)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                value.parse().map_err(de::Error::custom)
            }
        }
    };
}

runtime_id!(WorldId, "wld");
runtime_id!(ObjectId, "obj");
runtime_id!(ActionId, "act");
runtime_id!(EventId, "evt");
runtime_id!(SessionId, "ses");
runtime_id!(SaveId, "sav");
runtime_id!(GenerationId, "gen");
runtime_id!(NpcTurnRequestId, "ntr");

/// A semantic view of an [`ObjectId`] known to identify a character.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ActorId(ObjectId);

impl ActorId {
    #[must_use]
    pub const fn new(object_id: ObjectId) -> Self {
        Self(object_id)
    }

    #[must_use]
    pub const fn object_id(self) -> ObjectId {
        self.0
    }

    #[must_use]
    pub const fn as_object_id(&self) -> &ObjectId {
        &self.0
    }
}

impl From<ObjectId> for ActorId {
    fn from(value: ObjectId) -> Self {
        Self(value)
    }
}

impl From<ActorId> for ObjectId {
    fn from(value: ActorId) -> Self {
        value.0
    }
}

impl FromStr for ActorId {
    type Err = IdentityError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.parse().map(Self)
    }
}

impl fmt::Display for ActorId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, formatter)
    }
}

impl fmt::Debug for ActorId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModId(String);

impl ModId {
    pub const MIN_LEN: usize = 3;
    pub const MAX_LEN: usize = 127;

    pub fn parse(value: impl Into<String>) -> Result<Self, IdentityError> {
        let value = value.into();
        let segments = value.split('.').collect::<Vec<_>>();
        let valid = (Self::MIN_LEN..=Self::MAX_LEN).contains(&value.len())
            && value.is_ascii()
            && segments.len() >= 2
            && segments.iter().all(|segment| {
                !segment.is_empty()
                    && segment.len() <= 63
                    && segment
                        .as_bytes()
                        .first()
                        .is_some_and(u8::is_ascii_alphanumeric)
                    && segment
                        .as_bytes()
                        .last()
                        .is_some_and(u8::is_ascii_alphanumeric)
                    && segment.bytes().all(|byte| {
                        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
                    })
            });
        if !valid {
            return Err(IdentityError::InvalidModId);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for ModId {
    type Err = IdentityError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl fmt::Display for ModId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl fmt::Debug for ModId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl Serialize for ModId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ModId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::parse(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContentDefinitionId(String);

impl ContentDefinitionId {
    pub const MAX_LEN: usize = 255;

    pub fn new(mod_id: &ModId, kind: &str, local_key: &str) -> Result<Self, IdentityError> {
        Self::parse(format!("{mod_id}:{kind}/{local_key}"))
    }

    pub fn parse(value: impl Into<String>) -> Result<Self, IdentityError> {
        let value = value.into();
        let Some((mod_text, rest)) = value.split_once(':') else {
            return Err(IdentityError::InvalidContentDefinitionId);
        };
        let Some((kind, local_key)) = rest.split_once('/') else {
            return Err(IdentityError::InvalidContentDefinitionId);
        };
        let valid_kind = !kind.is_empty()
            && kind.len() <= 32
            && kind
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
        let valid_key = !local_key.is_empty()
            && local_key.len() <= 128
            && local_key.split('/').all(|segment| {
                !segment.is_empty()
                    && segment != "."
                    && segment != ".."
                    && segment.len() <= 64
                    && segment.bytes().all(|byte| {
                        byte.is_ascii_lowercase()
                            || byte.is_ascii_digit()
                            || byte == b'-'
                            || byte == b'_'
                    })
            });
        if value.len() > Self::MAX_LEN
            || !value.is_ascii()
            || ModId::parse(mod_text).is_err()
            || !valid_kind
            || !valid_key
        {
            return Err(IdentityError::InvalidContentDefinitionId);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn mod_id(&self) -> Result<ModId, IdentityError> {
        let (mod_id, _) = self
            .0
            .split_once(':')
            .ok_or(IdentityError::InvalidContentDefinitionId)?;
        ModId::parse(mod_id)
    }
}

impl FromStr for ContentDefinitionId {
    type Err = IdentityError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl fmt::Display for ContentDefinitionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl fmt::Debug for ContentDefinitionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl Serialize for ContentDefinitionId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ContentDefinitionId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::parse(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const V7: &str = "01890f6a-2b3c-7d4e-8f90-123456789abc";

    #[test]
    fn runtime_ids_require_canonical_prefixed_v7() {
        let id: ObjectId = format!("obj_{V7}").parse().expect("valid object id");
        assert_eq!(id.to_string(), format!("obj_{V7}"));
        assert!(format!("act_{V7}").parse::<ObjectId>().is_err());
        assert!(
            "obj_01890F6A-2B3C-7D4E-8F90-123456789ABC"
                .parse::<ObjectId>()
                .is_err()
        );
        assert!(
            "obj_550e8400-e29b-41d4-a716-446655440000"
                .parse::<ObjectId>()
                .is_err()
        );
    }

    #[test]
    fn actor_id_reuses_object_wire_identity() {
        let object: ObjectId = format!("obj_{V7}").parse().expect("valid object id");
        let actor = ActorId::from(object);
        assert_eq!(actor.to_string(), object.to_string());
        assert_eq!(
            serde_json::to_string(&actor).expect("serialize actor"),
            format!("\"{object}\"")
        );
    }

    #[test]
    fn content_identity_is_namespaced_and_bounded() {
        let mod_id = ModId::parse("games.loreloom.core").expect("valid mod id");
        let definition = ContentDefinitionId::new(&mod_id, "character", "harbor/warden")
            .expect("valid definition id");
        assert_eq!(
            definition.as_str(),
            "games.loreloom.core:character/harbor/warden"
        );
        assert_eq!(definition.mod_id().expect("extract mod id"), mod_id);
        assert!(ModId::parse("Loreloom.Core").is_err());
        assert!(ContentDefinitionId::parse("games.loreloom.core:character/../warden").is_err());
    }

    #[test]
    fn serde_revalidates_identifiers() {
        let value = format!("\"obj_{V7}\"");
        let id: ObjectId = serde_json::from_str(&value).expect("deserialize object id");
        assert_eq!(id.to_string(), format!("obj_{V7}"));
        assert!(serde_json::from_str::<ObjectId>("\"obj_invalid\"").is_err());
    }
}
