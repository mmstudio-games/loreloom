use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

use loreloom_core::ModId;
use semver::Version;
use serde::{Deserialize, Serialize};

use crate::error::AppError;

const MOD_SELECTION_SCHEMA_V1: u32 = 1;
const MAX_MOD_SELECTION_BYTES: usize = 65_536;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SelectedMod {
    pub mod_id: ModId,
    pub version: Version,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModSelectionFile {
    schema_version: u32,
    enabled: Vec<ModSelectionEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModSelectionEntry {
    mod_id: ModId,
    version: String,
}

pub fn load(world_root: &Path) -> Result<BTreeSet<SelectedMod>, AppError> {
    let directory = world_root.join(".loreloom");
    match fs::symlink_metadata(&directory) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(AppError::ModSelection("loadout directory is unsafe"));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(BTreeSet::new());
        }
        Err(error) => return Err(error.into()),
    }
    let path = directory.join("mods.toml");
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > u64::try_from(MAX_MOD_SELECTION_BYTES).unwrap_or(u64::MAX)
    {
        return Err(AppError::ModSelection("loadout file is unsafe"));
    }
    let bytes = fs::read(path)?;
    let text =
        std::str::from_utf8(&bytes).map_err(|_| AppError::ModSelection("loadout is not UTF-8"))?;
    let file = toml::from_str::<ModSelectionFile>(text).map_err(|_| AppError::ModSelectionCodec)?;
    if file.schema_version != MOD_SELECTION_SCHEMA_V1 {
        return Err(AppError::ModSelection(
            "loadout schema version is unsupported",
        ));
    }
    let mut enabled = BTreeSet::new();
    let mut mod_ids = BTreeSet::new();
    for entry in file.enabled {
        let selected = SelectedMod {
            mod_id: entry.mod_id,
            version: Version::parse(&entry.version)
                .map_err(|_| AppError::ModSelection("loadout version is invalid"))?,
        };
        if !mod_ids.insert(selected.mod_id.clone()) || !enabled.insert(selected) {
            return Err(AppError::ModSelection(
                "loadout contains duplicate Mod identities",
            ));
        }
    }
    Ok(enabled)
}

pub fn save(world_root: &Path, enabled: &BTreeSet<SelectedMod>) -> Result<(), AppError> {
    let mut mod_ids = BTreeSet::new();
    if enabled
        .iter()
        .any(|selected| !mod_ids.insert(selected.mod_id.clone()))
    {
        return Err(AppError::ModSelection(
            "loadout contains multiple versions of one Mod",
        ));
    }
    let directory = world_root.join(".loreloom");
    match fs::symlink_metadata(&directory) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(AppError::ModSelection("loadout directory is unsafe"));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(&directory)?;
        }
        Err(error) => return Err(error.into()),
    }
    let file = ModSelectionFile {
        schema_version: MOD_SELECTION_SCHEMA_V1,
        enabled: enabled
            .iter()
            .map(|selected| ModSelectionEntry {
                mod_id: selected.mod_id.clone(),
                version: selected.version.to_string(),
            })
            .collect(),
    };
    let encoded = toml::to_string(&file).map_err(|_| AppError::ModSelectionCodec)?;
    if encoded.len() > MAX_MOD_SELECTION_BYTES {
        return Err(AppError::ModSelection("loadout exceeds its size limit"));
    }
    let path = selection_path(world_root);
    let mut temporary = None;
    for nonce in 0_u8..16 {
        let candidate = directory.join(format!("mods.toml.tmp-{}-{nonce}", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => {
                temporary = Some((candidate, file));
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
    let Some((temporary, mut file)) = temporary else {
        return Err(AppError::ModSelection(
            "loadout temporary file could not be reserved",
        ));
    };
    if let Err(error) = file
        .write_all(encoded.as_bytes())
        .and_then(|()| file.sync_all())
    {
        drop(file);
        let _ = fs::remove_file(&temporary);
        return Err(error.into());
    }
    drop(file);
    if let Err(error) = fs::rename(&temporary, &path) {
        let _ = fs::remove_file(&temporary);
        return Err(error.into());
    }
    Ok(())
}

fn selection_path(world_root: &Path) -> std::path::PathBuf {
    world_root.join(".loreloom/mods.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selected(id: &str, version: &str) -> SelectedMod {
        SelectedMod {
            mod_id: ModId::parse(id).expect("Mod ID"),
            version: Version::parse(version).expect("version"),
        }
    }

    #[test]
    fn world_local_mod_selection_round_trips_in_stable_order() {
        let root = tempfile::tempdir().expect("world root");
        let enabled = BTreeSet::from([
            selected("games.loreloom.weather", "2.0.0"),
            selected("games.loreloom.characters", "1.1.0"),
        ]);

        save(root.path(), &enabled).expect("save loadout");

        assert_eq!(load(root.path()).expect("load loadout"), enabled);
        let encoded = fs::read_to_string(selection_path(root.path())).expect("loadout text");
        assert!(encoded.find("games.loreloom.characters") < encoded.find("games.loreloom.weather"));
    }

    #[test]
    fn malformed_mod_selection_does_not_become_an_enabled_set() {
        let root = tempfile::tempdir().expect("world root");
        fs::create_dir_all(root.path().join(".loreloom")).expect("state directory");
        fs::write(
            selection_path(root.path()),
            r#"schema_version = 1

[[enabled]]
mod_id = "games.loreloom.weather"
version = "not-semver"
"#,
        )
        .expect("invalid loadout");

        assert!(matches!(load(root.path()), Err(AppError::ModSelection(_))));
    }

    #[test]
    fn one_loadout_cannot_select_two_versions_of_the_same_mod() {
        let root = tempfile::tempdir().expect("world root");
        let enabled = BTreeSet::from([
            selected("games.loreloom.weather", "1.0.0"),
            selected("games.loreloom.weather", "2.0.0"),
        ]);

        assert!(matches!(
            save(root.path(), &enabled),
            Err(AppError::ModSelection(_))
        ));
        assert!(!selection_path(root.path()).exists());
    }
}
