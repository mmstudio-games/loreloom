use std::collections::{BTreeMap, BTreeSet};

use semver::Version;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{ContentDefinitionId, ContentHash, IdentityError, ModId, SaveId, WorldId};

pub const SAVE_FORMAT_V1: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PersistenceError {
    #[error("save format version is not supported")]
    UnsupportedSaveFormat,
    #[error("mod lock is invalid: {field}")]
    InvalidModLock { field: &'static str },
    #[error(transparent)]
    Identity(#[from] IdentityError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModSourceKind {
    Builtin,
    Directory,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedDependency {
    pub mod_id: ModId,
    pub version: Version,
    pub optional: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedMod {
    pub mod_id: ModId,
    pub version: Version,
    pub content_hash: ContentHash,
    pub manifest_schema: u32,
    pub content_schema: u32,
    pub source_kind: ModSourceKind,
    pub dependencies: Vec<LockedDependency>,
    pub applied_patches: Vec<ContentDefinitionId>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModLock {
    pub mods: Vec<LockedMod>,
}

impl ModLock {
    pub fn validate(&self) -> Result<(), PersistenceError> {
        let positions = self.mods.iter().enumerate().try_fold(
            BTreeMap::new(),
            |mut positions, (index, locked)| {
                if positions.insert(locked.mod_id.clone(), index).is_some() {
                    return invalid("duplicate_mod");
                }
                Ok(positions)
            },
        )?;

        for (index, locked) in self.mods.iter().enumerate() {
            if locked.manifest_schema == 0 || locked.content_schema == 0 {
                return invalid("schema_version");
            }
            let mut previous_dependency = None;
            for dependency in &locked.dependencies {
                if previous_dependency
                    .as_ref()
                    .is_some_and(|previous| previous >= &dependency.mod_id)
                {
                    return invalid("dependency_order");
                }
                previous_dependency = Some(dependency.mod_id.clone());
                match positions.get(&dependency.mod_id).copied() {
                    Some(dependency_index) if dependency_index < index => {
                        if self.mods[dependency_index].version != dependency.version {
                            return invalid("dependency_version");
                        }
                    }
                    Some(_) => return invalid("dependency_topology"),
                    None if !dependency.optional => return invalid("required_dependency"),
                    None => {}
                }
            }

            let mut patches = BTreeSet::new();
            for patch in &locked.applied_patches {
                if patch.mod_id()? != locked.mod_id || !patches.insert(patch.clone()) {
                    return invalid("applied_patch");
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveManifest {
    pub format_version: u32,
    pub save_id: SaveId,
    pub world_id: WorldId,
    pub mod_lock: ModLock,
}

impl SaveManifest {
    pub fn validate(&self) -> Result<(), PersistenceError> {
        if self.format_version != SAVE_FORMAT_V1 {
            return Err(PersistenceError::UnsupportedSaveFormat);
        }
        self.mod_lock.validate()
    }
}

fn invalid<T>(field: &'static str) -> Result<T, PersistenceError> {
    Err(PersistenceError::InvalidModLock { field })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn locked(mod_id: &str, dependencies: Vec<LockedDependency>) -> LockedMod {
        let mod_id = ModId::parse(mod_id).expect("mod id");
        LockedMod {
            applied_patches: vec![
                ContentDefinitionId::new(&mod_id, "patch", "base").expect("patch id"),
            ],
            mod_id,
            version: Version::new(1, 0, 0),
            content_hash: ContentHash::parse("a".repeat(64)).expect("hash"),
            manifest_schema: 1,
            content_schema: 1,
            source_kind: ModSourceKind::Directory,
            dependencies,
        }
    }

    #[test]
    fn mod_lock_requires_exact_topological_dependencies() {
        let base = locked("games.loreloom.base", Vec::new());
        let extension = locked(
            "games.loreloom.extension",
            vec![LockedDependency {
                mod_id: base.mod_id.clone(),
                version: base.version.clone(),
                optional: false,
            }],
        );
        let lock = ModLock {
            mods: vec![base.clone(), extension.clone()],
        };
        lock.validate().expect("canonical lock");

        assert!(matches!(
            ModLock {
                mods: vec![extension, base],
            }
            .validate(),
            Err(PersistenceError::InvalidModLock {
                field: "dependency_topology"
            })
        ));
    }

    #[test]
    fn save_manifest_round_trips_and_rejects_unknown_fields() {
        let manifest = SaveManifest {
            format_version: SAVE_FORMAT_V1,
            save_id: "sav_01890f6a-2b3c-7d4e-8f90-123456789abc"
                .parse()
                .expect("save id"),
            world_id: "wld_01890f6a-2b3d-7d4e-8f90-123456789abc"
                .parse()
                .expect("world id"),
            mod_lock: ModLock {
                mods: vec![locked("games.loreloom.base", Vec::new())],
            },
        };
        manifest.validate().expect("manifest");
        let encoded = serde_json::to_value(&manifest).expect("encode manifest");
        assert_eq!(
            serde_json::from_value::<SaveManifest>(encoded).expect("decode manifest"),
            manifest
        );

        let mut unknown = serde_json::to_value(&manifest).expect("encode manifest");
        unknown
            .as_object_mut()
            .expect("manifest object")
            .insert("path".into(), serde_json::Value::String("private".into()));
        assert!(serde_json::from_value::<SaveManifest>(unknown).is_err());
    }
}
