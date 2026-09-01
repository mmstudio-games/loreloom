use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use loreloom_core::{ContentDefinitionId, LockedMod, LongText, ModId, ModLock, WorldLock};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    CONTENT_SCHEMA_V1, LORELOOM_ENGINE_VERSION, MOD_MANIFEST_SCHEMA_V1, ModCapability,
    ModManifestDraft, PackageError, PackagePayload, PromptManifest, VirtualPackage,
};

pub const WORLD_MANIFEST_SCHEMA_V1: u32 = 1;
const WORLD_MANIFEST_MAX_BYTES: usize = 262_144;
const WORLD_FILE_MAX_BYTES: usize = 1_048_576;
const WORLD_TOTAL_MAX_BYTES: usize = 16_777_216;
const WORLD_FILE_MAXIMUM: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldManifest {
    pub schema_version: u32,
    pub world_id: ModId,
    pub version: Version,
    pub engine: VersionReq,
    pub content_schema: u32,
    pub initial_scene: ContentDefinitionId,
    pub inventory_root_definition: ContentDefinitionId,
    pub spawn_system_definition: ContentDefinitionId,
    pub npc_generation_policy: ContentDefinitionId,
    #[serde(default)]
    pub player_creation: PlayerCreationMode,
    pub content: Vec<String>,
    #[serde(default)]
    pub rules: Vec<String>,
    #[serde(default)]
    pub resources: Vec<String>,
    pub prompts: PromptManifest,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlayerCreationMode {
    #[default]
    Fixed,
    Preset {
        characters: Vec<ContentDefinitionId>,
    },
    Ugc {
        form_id: ContentDefinitionId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldProjectSource {
    manifest: WorldManifest,
    package: VirtualPackage,
}

impl WorldProjectSource {
    pub fn load(root: impl AsRef<Path>) -> Result<Self, WorldProjectError> {
        let root = root.as_ref();
        require_directory(root)?;
        let manifest_bytes = read_regular_file(&root.join("world.toml"), WORLD_MANIFEST_MAX_BYTES)?;
        let manifest_text =
            std::str::from_utf8(&manifest_bytes).map_err(|_| WorldProjectError::ManifestCodec)?;
        let manifest: WorldManifest =
            toml::from_str(manifest_text).map_err(|_| WorldProjectError::ManifestCodec)?;
        validate_manifest(&manifest)?;

        let mut payload_paths = manifest.content.clone();
        payload_paths.extend(manifest.rules.iter().cloned());
        payload_paths.extend(manifest.resources.iter().cloned());
        payload_paths.extend(manifest.prompts.narrator.iter().cloned());
        payload_paths.extend(manifest.prompts.npc.iter().cloned());
        let mut unique = BTreeSet::new();
        if payload_paths.len() > WORLD_FILE_MAXIMUM
            || payload_paths
                .iter()
                .any(|path| !unique.insert(path.clone()))
        {
            return Err(WorldProjectError::InvalidManifest { field: "payloads" });
        }
        let mut payloads = Vec::with_capacity(payload_paths.len() + 1);
        let mut total_bytes = manifest_bytes.len();
        for path in payload_paths {
            validate_world_path(&path)?;
            let bytes = read_regular_file(&root.join(&path), WORLD_FILE_MAX_BYTES)?;
            total_bytes = total_bytes.saturating_add(bytes.len());
            if total_bytes > WORLD_TOTAL_MAX_BYTES {
                return Err(WorldProjectError::ResourceLimit);
            }
            payloads.push(PackagePayload::new(path, bytes));
        }
        payloads.push(PackagePayload::new(
            "assets/loreloom-world.toml",
            manifest_bytes,
        ));
        for path in manifest
            .prompts
            .narrator
            .iter()
            .chain(&manifest.prompts.npc)
        {
            let prompt_bytes = payloads
                .iter()
                .find(|payload| &payload.path == path)
                .map(|payload| payload.bytes.as_slice())
                .ok_or(WorldProjectError::InvalidManifest { field: "prompts" })?;
            let prompt =
                std::str::from_utf8(prompt_bytes).map_err(|_| WorldProjectError::InvalidPrompt)?;
            LongText::non_empty(prompt).map_err(|_| WorldProjectError::InvalidPrompt)?;
        }
        let mut capabilities = vec![ModCapability::Content];
        if !manifest.rules.is_empty() {
            capabilities.push(ModCapability::Rules);
        }
        let package = VirtualPackage::builtin(
            ModManifestDraft {
                schema_version: MOD_MANIFEST_SCHEMA_V1,
                mod_id: manifest.world_id.clone(),
                version: manifest.version.clone(),
                pack_id: ContentDefinitionId::new(&manifest.world_id, "pack", "world")
                    .map_err(|_| WorldProjectError::InvalidManifest { field: "world_id" })?,
                engine: manifest.engine.clone(),
                content_schema: manifest.content_schema,
                dependencies: Vec::new(),
                capabilities,
                patches: Vec::new(),
                prompts: manifest.prompts.clone(),
            },
            payloads,
        )?;
        Ok(Self { manifest, package })
    }

    #[must_use]
    pub fn manifest(&self) -> &WorldManifest {
        &self.manifest
    }

    #[must_use]
    pub(crate) fn package(&self) -> &VirtualPackage {
        &self.package
    }

    pub(crate) fn split_lock(
        &self,
        full_lock: ModLock,
        engine_namespaces: &BTreeSet<ModId>,
    ) -> Result<(WorldLock, ModLock), WorldProjectError> {
        split_world_lock(full_lock, &self.manifest.world_id, engine_namespaces)
    }
}

#[derive(Debug, Error)]
pub enum WorldProjectError {
    #[error("world root or payload I/O failed")]
    Io(#[source] std::io::Error),
    #[error("world root contains a symbolic link where a regular file or directory is required")]
    Symlink,
    #[error("world manifest is not valid UTF-8 TOML")]
    ManifestCodec,
    #[error("world manifest field is invalid: {field}")]
    InvalidManifest { field: &'static str },
    #[error("world payload path is unsafe or belongs to an unsupported directory")]
    UnsafePath,
    #[error("world payload exceeds the product resource limits")]
    ResourceLimit,
    #[error("world agent prompt is empty, invalid UTF-8, or too large")]
    InvalidPrompt,
    #[error("compiled world lock is invalid")]
    InvalidLock,
    #[error(transparent)]
    Package(#[from] PackageError),
}

fn validate_manifest(manifest: &WorldManifest) -> Result<(), WorldProjectError> {
    let engine_version = Version::parse(LORELOOM_ENGINE_VERSION).map_err(|_| {
        WorldProjectError::InvalidManifest {
            field: "engine_version",
        }
    })?;
    if manifest.schema_version != WORLD_MANIFEST_SCHEMA_V1 {
        return invalid_manifest("schema_version");
    }
    if manifest.content_schema != CONTENT_SCHEMA_V1 {
        return invalid_manifest("content_schema");
    }
    if !manifest.engine.matches(&engine_version) {
        return invalid_manifest("engine");
    }
    if manifest.content.is_empty() {
        return invalid_manifest("content");
    }
    for (field, id, expected_kind) in [
        ("initial_scene", &manifest.initial_scene, "scene"),
        (
            "inventory_root_definition",
            &manifest.inventory_root_definition,
            "item",
        ),
        (
            "spawn_system_definition",
            &manifest.spawn_system_definition,
            "system",
        ),
        (
            "npc_generation_policy",
            &manifest.npc_generation_policy,
            "generation_policy",
        ),
    ] {
        if id
            .mod_id()
            .map_err(|_| WorldProjectError::InvalidManifest { field })?
            != manifest.world_id
            || id
                .kind()
                .map_err(|_| WorldProjectError::InvalidManifest { field })?
                != expected_kind
        {
            return invalid_manifest(field);
        }
    }
    match &manifest.player_creation {
        PlayerCreationMode::Fixed => {}
        PlayerCreationMode::Preset { characters } => {
            let unique = characters.iter().collect::<BTreeSet<_>>();
            if characters.is_empty()
                || unique.len() != characters.len()
                || characters
                    .iter()
                    .any(|id| id.kind().ok() != Some("character"))
            {
                return invalid_manifest("player_creation.characters");
            }
        }
        PlayerCreationMode::Ugc { form_id }
            if form_id.kind().ok() != Some("player_creation_form") =>
        {
            return invalid_manifest("player_creation.form_id");
        }
        PlayerCreationMode::Ugc { .. } => {}
    }
    for path in &manifest.content {
        if !path.starts_with("content/") || !path.ends_with(".json") {
            return invalid_manifest("content");
        }
    }
    for path in &manifest.rules {
        if !path.starts_with("rules/") || !path.ends_with(".json") {
            return invalid_manifest("rules");
        }
    }
    for path in &manifest.resources {
        let valid = (path.starts_with("locales/") && path.ends_with(".json"))
            || path.starts_with("assets/")
            || (path.starts_with("prompts/") && path.ends_with(".md"));
        if !valid {
            return invalid_manifest("resources");
        }
    }
    if manifest.prompts.narrator.is_empty() {
        return invalid_manifest("prompts.narrator");
    }
    for path in manifest
        .prompts
        .narrator
        .iter()
        .chain(&manifest.prompts.npc)
    {
        if !path.starts_with("prompts/") || !path.ends_with(".md") {
            return invalid_manifest("prompts");
        }
    }
    Ok(())
}

fn split_world_lock(
    full_lock: ModLock,
    world_id: &ModId,
    engine_namespaces: &BTreeSet<ModId>,
) -> Result<(WorldLock, ModLock), WorldProjectError> {
    let mut world = None;
    let excluded = engine_namespaces
        .iter()
        .chain(std::iter::once(world_id))
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut mods = Vec::new();
    for locked in full_lock.mods {
        if &locked.mod_id == world_id {
            if world.is_some()
                || !locked.dependencies.is_empty()
                || !locked.applied_patches.is_empty()
            {
                return Err(WorldProjectError::InvalidLock);
            }
            world = Some(WorldLock {
                world_id: locked.mod_id,
                version: locked.version,
                content_hash: locked.content_hash,
                manifest_schema: WORLD_MANIFEST_SCHEMA_V1,
                content_schema: locked.content_schema,
            });
        } else if !engine_namespaces.contains(&locked.mod_id) {
            mods.push(LockedMod {
                dependencies: locked
                    .dependencies
                    .into_iter()
                    .filter(|dependency| !excluded.contains(&dependency.mod_id))
                    .collect(),
                ..locked
            });
        }
    }
    let world = world.ok_or(WorldProjectError::InvalidLock)?;
    world
        .validate()
        .map_err(|_| WorldProjectError::InvalidLock)?;
    let mod_lock = ModLock { mods };
    mod_lock
        .validate()
        .map_err(|_| WorldProjectError::InvalidLock)?;
    Ok((world, mod_lock))
}

fn require_directory(path: &Path) -> Result<(), WorldProjectError> {
    let metadata = fs::symlink_metadata(path).map_err(WorldProjectError::Io)?;
    if metadata.file_type().is_symlink() {
        return Err(WorldProjectError::Symlink);
    }
    if !metadata.is_dir() {
        return Err(WorldProjectError::UnsafePath);
    }
    Ok(())
}

fn read_regular_file(path: &Path, maximum: usize) -> Result<Vec<u8>, WorldProjectError> {
    let metadata = fs::symlink_metadata(path).map_err(WorldProjectError::Io)?;
    if metadata.file_type().is_symlink() {
        return Err(WorldProjectError::Symlink);
    }
    if !metadata.is_file() {
        return Err(WorldProjectError::UnsafePath);
    }
    if metadata.len() > u64::try_from(maximum).map_err(|_| WorldProjectError::ResourceLimit)? {
        return Err(WorldProjectError::ResourceLimit);
    }
    fs::read(path).map_err(WorldProjectError::Io)
}

fn validate_world_path(path: &str) -> Result<(), WorldProjectError> {
    let candidate = PathBuf::from(path);
    let valid_prefix = path.starts_with("content/")
        || path.starts_with("rules/")
        || path.starts_with("prompts/")
        || path.starts_with("locales/")
        || path.starts_with("assets/");
    if !valid_prefix
        || candidate.is_absolute()
        || path.contains('\\')
        || path.contains('\0')
        || path
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(WorldProjectError::UnsafePath);
    }
    Ok(())
}

fn invalid_manifest<T>(field: &'static str) -> Result<T, WorldProjectError> {
    Err(WorldProjectError::InvalidManifest { field })
}

#[cfg(test)]
mod tests {
    use loreloom_core::{ContentHash, LockedDependency, ModSourceKind};
    use tempfile::TempDir;

    use super::*;
    use crate::{PackageCompiler, PackageSource};

    fn write_world(root: &Path, prompt: &str) {
        fs::create_dir_all(root.join("content")).expect("content directory");
        fs::create_dir_all(root.join("prompts")).expect("prompt directory");
        fs::create_dir_all(root.join("mods/unlisted")).expect("unlisted Mod directory");
        fs::write(
            root.join("world.toml"),
            r#"schema_version = 1
world_id = "games.loreloom.test-world"
version = "0.1.0"
engine = "=0.1.0"
content_schema = 1
initial_scene = "games.loreloom.test-world:scene/start"
inventory_root_definition = "games.loreloom.test-world:item/inventory"
spawn_system_definition = "games.loreloom.test-world:system/spawn"
npc_generation_policy = "games.loreloom.test-world:generation_policy/default"
content = ["content/world.json"]
rules = []
resources = []

[prompts]
narrator = ["prompts/narrator.md"]
npc = ["prompts/npc.md"]
"#,
        )
        .expect("world manifest");
        fs::write(
            root.join("content/world.json"),
            r#"{"schema_version":1,"definitions":[]}"#,
        )
        .expect("world content");
        fs::write(root.join("prompts/narrator.md"), prompt).expect("world prompt");
        fs::write(root.join("prompts/npc.md"), "Act from what you know.")
            .expect("world NPC prompt");
        fs::write(root.join("mods/unlisted/ignored.txt"), "not enabled")
            .expect("unlisted Mod file");
    }

    fn compile_world(
        root: &Path,
    ) -> (
        WorldProjectSource,
        WorldLock,
        ModLock,
        crate::CompiledAgentPrompts,
    ) {
        let source = WorldProjectSource::load(root).expect("load world source");
        let compiled = PackageCompiler::default()
            .compile_world(
                &source,
                std::iter::empty::<PackageSource>(),
                std::iter::empty::<PackageSource>(),
                &BTreeSet::new(),
            )
            .expect("compile world package");
        let prompts = compiled.prompts().clone();
        let (_, world_lock, mod_lock, _) = compiled.into_parts();
        (source, world_lock, mod_lock, prompts)
    }

    #[test]
    fn declared_prompt_is_loaded_and_participates_in_world_lock() {
        let first = TempDir::new().expect("first world root");
        write_world(first.path(), "用中文叙述。\n");
        let (_, first_lock, first_mods, prompts) = compile_world(first.path());
        assert_eq!(
            prompts
                .narrator()
                .iter()
                .map(LongText::as_str)
                .collect::<Vec<_>>(),
            ["用中文叙述。\n"]
        );
        assert_eq!(
            prompts
                .npc()
                .iter()
                .map(LongText::as_str)
                .collect::<Vec<_>>(),
            ["Act from what you know."]
        );
        assert!(first_mods.mods.is_empty());

        let second = TempDir::new().expect("second world root");
        write_world(second.path(), "用中文叙述，并保持克制。\n");
        let (_, second_lock, _, _) = compile_world(second.path());
        assert_ne!(first_lock.content_hash, second_lock.content_hash);

        fs::write(
            first.path().join("mods/unlisted/ignored.txt"),
            "changed but still not enabled",
        )
        .expect("change unlisted Mod");
        let (_, unchanged_lock, _, _) = compile_world(first.path());
        assert_eq!(first_lock, unchanged_lock);
    }

    fn prompt_mod(
        id: &str,
        dependency: Option<&str>,
        narrator: &[&str],
        npc: &[&str],
        undeclared: Option<&str>,
    ) -> VirtualPackage {
        let mod_id = ModId::parse(id).expect("Mod ID");
        let narrator_paths = narrator
            .iter()
            .enumerate()
            .map(|(index, _)| format!("prompts/narrator-{index}.md"))
            .collect::<Vec<_>>();
        let npc_paths = npc
            .iter()
            .enumerate()
            .map(|(index, _)| format!("prompts/npc-{index}.md"))
            .collect::<Vec<_>>();
        let mut payloads = narrator_paths
            .iter()
            .zip(narrator)
            .chain(npc_paths.iter().zip(npc))
            .map(|(path, text)| PackagePayload::new(path, text.as_bytes()))
            .collect::<Vec<_>>();
        if let Some(text) = undeclared {
            payloads.push(PackagePayload::new("prompts/unused.md", text.as_bytes()));
        }
        VirtualPackage::builtin(
            ModManifestDraft {
                schema_version: MOD_MANIFEST_SCHEMA_V1,
                pack_id: ContentDefinitionId::new(&mod_id, "pack", "main").expect("Pack ID"),
                mod_id,
                version: Version::new(1, 0, 0),
                engine: VersionReq::parse("=0.1.0").expect("Engine requirement"),
                content_schema: CONTENT_SCHEMA_V1,
                dependencies: dependency
                    .map(|dependency| crate::ModDependency {
                        mod_id: ModId::parse(dependency).expect("dependency Mod ID"),
                        requirement: VersionReq::parse("=1.0.0").expect("dependency requirement"),
                        optional: false,
                    })
                    .into_iter()
                    .collect(),
                capabilities: vec![ModCapability::Content],
                patches: Vec::new(),
                prompts: PromptManifest {
                    narrator: narrator_paths,
                    npc: npc_paths,
                },
            },
            payloads,
        )
        .expect("sealed prompt Mod")
    }

    #[test]
    fn mod_prompts_append_in_dependency_and_manifest_order_without_injecting_resources() {
        let root = TempDir::new().expect("world root");
        write_world(root.path(), "World narrator.");
        let source = WorldProjectSource::load(root.path()).expect("load world source");
        let base = prompt_mod(
            "games.loreloom.context-base",
            None,
            &["Base narrator one.", "Base narrator two."],
            &["Base NPC."],
            Some("This resource must not be injected."),
        );
        let addon = prompt_mod(
            "games.loreloom.context-addon",
            Some("games.loreloom.context-base"),
            &["Addon narrator."],
            &["Addon NPC one.", "Addon NPC two."],
            None,
        );
        let compiled = PackageCompiler::default()
            .compile_world(
                &source,
                std::iter::empty::<PackageSource>(),
                [PackageSource::Builtin(addon), PackageSource::Builtin(base)],
                &BTreeSet::new(),
            )
            .expect("compile prompt Mods");

        assert_eq!(
            compiled
                .prompts()
                .narrator()
                .iter()
                .map(LongText::as_str)
                .collect::<Vec<_>>(),
            [
                "World narrator.",
                "Base narrator one.",
                "Base narrator two.",
                "Addon narrator."
            ]
        );
        assert_eq!(
            compiled
                .prompts()
                .npc()
                .iter()
                .map(LongText::as_str)
                .collect::<Vec<_>>(),
            [
                "Act from what you know.",
                "Base NPC.",
                "Addon NPC one.",
                "Addon NPC two."
            ]
        );
        assert_eq!(
            compiled
                .mod_lock()
                .mods
                .iter()
                .map(|locked| locked.mod_id.as_str())
                .collect::<Vec<_>>(),
            [
                "games.loreloom.context-base",
                "games.loreloom.context-addon"
            ]
        );
    }

    #[test]
    fn split_lock_removes_world_and_engine_from_extension_mod_lock() {
        let root = TempDir::new().expect("world root");
        write_world(root.path(), "Narrate the test world.");
        let source = WorldProjectSource::load(root.path()).expect("load world source");
        let engine_id = ModId::parse("games.loreloom.core").expect("engine ID");
        let extension_id = ModId::parse("games.loreloom.extension").expect("extension ID");
        let world_id = source.manifest().world_id.clone();
        let locked = |mod_id: ModId, marker: char, dependencies: Vec<LockedDependency>| LockedMod {
            mod_id,
            version: Version::new(1, 0, 0),
            content_hash: ContentHash::parse(marker.to_string().repeat(64)).expect("content hash"),
            manifest_schema: 1,
            content_schema: 1,
            source_kind: ModSourceKind::Builtin,
            dependencies,
            applied_patches: Vec::new(),
        };
        let full_lock = ModLock {
            mods: vec![
                locked(engine_id.clone(), 'a', Vec::new()),
                locked(world_id.clone(), 'b', Vec::new()),
                locked(
                    extension_id.clone(),
                    'c',
                    vec![
                        LockedDependency {
                            mod_id: engine_id.clone(),
                            version: Version::new(1, 0, 0),
                            optional: false,
                        },
                        LockedDependency {
                            mod_id: world_id.clone(),
                            version: Version::new(1, 0, 0),
                            optional: false,
                        },
                    ],
                ),
            ],
        };
        full_lock.validate().expect("full lock");

        let (world_lock, mod_lock) = source
            .split_lock(full_lock, &BTreeSet::from([engine_id]))
            .expect("split locks");
        assert_eq!(world_lock.world_id, world_id);
        assert_eq!(mod_lock.mods.len(), 1);
        assert_eq!(mod_lock.mods[0].mod_id, extension_id);
        assert!(mod_lock.mods[0].dependencies.is_empty());
    }

    #[test]
    fn unsafe_or_undeclared_world_payloads_are_rejected_or_ignored() {
        let root = TempDir::new().expect("world root");
        write_world(root.path(), "Narrate.");
        fs::write(
            root.path().join("world.toml"),
            fs::read_to_string(root.path().join("world.toml"))
                .expect("read manifest")
                .replace("content/world.json", "../outside.json"),
        )
        .expect("unsafe manifest");
        assert!(matches!(
            WorldProjectSource::load(root.path()),
            Err(WorldProjectError::InvalidManifest { field: "content" })
                | Err(WorldProjectError::UnsafePath)
        ));
    }
}
