use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use loreloom_core::{ModId, SaveId};
use serde::{Deserialize, Serialize};

use crate::error::AppError;

const CATALOG_SCHEMA_V1: u32 = 1;
const SIDECAR_SUFFIX: &str = ".loreloom-save.toml";
const MAX_SIDECAR_BYTES: u64 = 65_536;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveCatalogEntry {
    pub path: PathBuf,
    pub save_id: SaveId,
    pub display_name: String,
    pub last_used_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SaveCatalogSidecar {
    schema_version: u32,
    save_id: SaveId,
    world_id: ModId,
    display_name: String,
    last_used_at: u64,
}

pub fn scan(world_root: &Path, world_id: &ModId) -> Vec<SaveCatalogEntry> {
    let root = world_root.join(".loreloom");
    let mut entries = Vec::new();
    scan_directory(&root, world_id, &mut entries);
    scan_directory(&root.join("saves"), world_id, &mut entries);
    let mut seen = BTreeSet::new();
    entries.retain(|entry| seen.insert(entry.path.clone()));
    entries.sort_by(|left, right| {
        right
            .last_used_at
            .cmp(&left.last_used_at)
            .then_with(|| left.display_name.cmp(&right.display_name))
            .then_with(|| left.path.cmp(&right.path))
    });
    entries
}

fn scan_directory(directory: &Path, world_id: &ModId, entries: &mut Vec<SaveCatalogEntry>) {
    let Ok(metadata) = fs::symlink_metadata(directory) else {
        return;
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return;
    }
    let Ok(children) = fs::read_dir(directory) else {
        return;
    };
    for child in children.flatten() {
        let path = child.path();
        let Ok(file_type) = child.file_type() else {
            continue;
        };
        if file_type.is_symlink() || !file_type.is_file() {
            continue;
        }
        let Ok(metadata) = child.metadata() else {
            continue;
        };
        if metadata.len() > MAX_SIDECAR_BYTES {
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(save_name) = file_name.strip_suffix(SIDECAR_SUFFIX) else {
            continue;
        };
        if save_name.is_empty() {
            continue;
        }
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        let Ok(text) = std::str::from_utf8(&bytes) else {
            continue;
        };
        let Ok(sidecar) = toml::from_str::<SaveCatalogSidecar>(text) else {
            continue;
        };
        if sidecar.schema_version != CATALOG_SCHEMA_V1
            || sidecar.world_id != *world_id
            || !safe_display_name(&sidecar.display_name)
        {
            continue;
        }
        let save_path = directory.join(save_name);
        let Ok(save_metadata) = fs::symlink_metadata(&save_path) else {
            continue;
        };
        if save_metadata.file_type().is_symlink() || !save_metadata.is_dir() {
            continue;
        }
        entries.push(SaveCatalogEntry {
            path: save_path,
            save_id: sidecar.save_id,
            display_name: sidecar.display_name,
            last_used_at: sidecar.last_used_at,
        });
    }
}

pub fn register(
    save_path: &Path,
    save_id: SaveId,
    world_id: ModId,
    display_name: &str,
) -> Result<(), AppError> {
    if !safe_display_name(display_name) {
        return Err(AppError::SaveCatalog("save display name is invalid"));
    }
    let sidecar = SaveCatalogSidecar {
        schema_version: CATALOG_SCHEMA_V1,
        save_id,
        world_id,
        display_name: display_name.to_owned(),
        last_used_at: unix_seconds(),
    };
    let encoded = toml::to_string(&sidecar).map_err(|_| AppError::SaveCatalogCodec)?;
    let path = sidecar_path(save_path);
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    let mut temporary_name = path.as_os_str().to_owned();
    temporary_name.push(format!(".tmp-{}", std::process::id()));
    let temporary = PathBuf::from(temporary_name);
    fs::write(&temporary, encoded)?;
    if let Err(error) = fs::rename(&temporary, &path) {
        let _ = fs::remove_file(&temporary);
        return Err(AppError::Io(error));
    }
    Ok(())
}

pub fn new_save_path(world_root: &Path) -> Result<PathBuf, AppError> {
    let saves = world_root.join(".loreloom/saves");
    let timestamp = unix_seconds();
    let mut suffix = 0_u32;
    loop {
        let name = if suffix == 0 {
            format!("save-{timestamp}")
        } else {
            format!("save-{timestamp}-{suffix}")
        };
        let path = saves.join(&name);
        if !path.exists() && !sidecar_path(&path).exists() {
            return Ok(path);
        }
        suffix = suffix
            .checked_add(1)
            .ok_or(AppError::SaveCatalog("save path namespace is exhausted"))?;
    }
}

fn sidecar_path(save_path: &Path) -> PathBuf {
    let mut path = OsString::from(save_path.as_os_str());
    path.push(SIDECAR_SUFFIX);
    PathBuf::from(path)
}

fn safe_display_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && !value.chars().any(char::is_control)
        && value.trim() == value
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecars_filter_worlds_and_sort_without_opening_store_directories() {
        let root = tempfile::tempdir().expect("world root");
        let first = root.path().join(".loreloom/saves/first");
        let second = root.path().join(".loreloom/saves/second");
        fs::create_dir_all(&first).expect("first save");
        fs::create_dir_all(&second).expect("second save");
        let world_id = ModId::parse("games.loreloom.test").expect("world id");
        let other_id = ModId::parse("games.loreloom.other").expect("other id");
        let first_id: SaveId = "sav_01890f6a-2b3c-7d4e-8f90-123456789abc"
            .parse()
            .expect("save id");
        let second_id: SaveId = "sav_01890f6a-2b3d-7d4e-8f90-123456789abc"
            .parse()
            .expect("save id");
        write_test_sidecar(&first, first_id, world_id.clone(), "First", 1);
        write_test_sidecar(&second, second_id, world_id.clone(), "Second", 2);
        let foreign = root.path().join(".loreloom/saves/foreign");
        fs::create_dir_all(&foreign).expect("foreign save");
        write_test_sidecar(&foreign, first_id, other_id, "Foreign", 3);

        let catalog = scan(root.path(), &world_id);

        assert_eq!(catalog.len(), 2);
        assert_eq!(catalog[0].path, second);
        assert_eq!(catalog[1].path, first);
    }

    #[test]
    fn register_publishes_a_discoverable_atomic_sidecar() {
        let root = tempfile::tempdir().expect("world root");
        let save_path = root.path().join(".loreloom/saves/created");
        fs::create_dir_all(&save_path).expect("save directory");
        let world_id = ModId::parse("games.loreloom.test").expect("world id");
        let save_id: SaveId = "sav_01890f6a-2b3c-7d4e-8f90-123456789abc"
            .parse()
            .expect("save id");

        register(&save_path, save_id, world_id.clone(), "Created Save").expect("register");
        let catalog = scan(root.path(), &world_id);

        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog[0].save_id, save_id);
        assert_eq!(catalog[0].display_name, "Created Save");
        assert!(sidecar_path(&save_path).is_file());
    }

    #[test]
    fn new_save_path_skips_an_existing_save_without_creating_files() {
        let root = tempfile::tempdir().expect("world root");
        let first = new_save_path(root.path()).expect("first candidate");
        fs::create_dir_all(&first).expect("existing save");

        let second = new_save_path(root.path()).expect("second candidate");

        assert_ne!(second, first);
        assert!(!second.exists());
        assert!(!sidecar_path(&second).exists());
    }

    fn write_test_sidecar(
        save_path: &Path,
        save_id: SaveId,
        world_id: ModId,
        display_name: &str,
        last_used_at: u64,
    ) {
        let sidecar = SaveCatalogSidecar {
            schema_version: CATALOG_SCHEMA_V1,
            save_id,
            world_id,
            display_name: display_name.to_owned(),
            last_used_at,
        };
        fs::write(
            sidecar_path(save_path),
            toml::to_string(&sidecar).expect("sidecar"),
        )
        .expect("write sidecar");
    }
}
