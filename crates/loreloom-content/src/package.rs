use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
};

use loreloom_core::{
    ContentDefinitionId, ContentHash, LockedDependency, LockedMod, LongText, ModId, ModLock,
    ModSourceKind, PackageContentView, WorldLock,
};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    CONTENT_SCHEMA_V1, ContentDocument, ContentError, ContentPackContext, Definition,
    DefinitionRegistry, WorldProjectError, WorldProjectSource,
};

pub const MOD_MANIFEST_SCHEMA_V1: u32 = 1;
pub const LORELOOM_ENGINE_VERSION: &str = "0.1.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModCapability {
    Content,
    Rules,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModDependency {
    pub mod_id: ModId,
    pub requirement: VersionReq,
    #[serde(default)]
    pub optional: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatchDeclaration {
    pub id: ContentDefinitionId,
    pub file: String,
    pub target_mod: ModId,
    pub target_version: VersionReq,
    pub target_definition: ContentDefinitionId,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromptManifest {
    #[serde(default)]
    pub narrator: Vec<String>,
    #[serde(default)]
    pub npc: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModManifest {
    pub schema_version: u32,
    pub mod_id: ModId,
    pub version: Version,
    pub pack_id: ContentDefinitionId,
    pub engine: VersionReq,
    pub content_schema: u32,
    #[serde(default)]
    pub dependencies: Vec<ModDependency>,
    pub capabilities: Vec<ModCapability>,
    #[serde(default)]
    pub patches: Vec<PatchDeclaration>,
    #[serde(default)]
    pub prompts: PromptManifest,
    pub content_hash: ContentHash,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectedPackage {
    manifest: ModManifest,
    content: PackageContentView,
}

impl InspectedPackage {
    #[must_use]
    pub fn manifest(&self) -> &ModManifest {
        &self.manifest
    }

    #[must_use]
    pub fn content(&self) -> &PackageContentView {
        &self.content
    }

    #[must_use]
    pub fn into_parts(self) -> (ModManifest, PackageContentView) {
        (self.manifest, self.content)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModManifestDraft {
    pub schema_version: u32,
    pub mod_id: ModId,
    pub version: Version,
    pub pack_id: ContentDefinitionId,
    pub engine: VersionReq,
    pub content_schema: u32,
    pub dependencies: Vec<ModDependency>,
    pub capabilities: Vec<ModCapability>,
    pub patches: Vec<PatchDeclaration>,
    pub prompts: PromptManifest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackagePayload {
    pub path: String,
    pub bytes: Vec<u8>,
}

impl PackagePayload {
    #[must_use]
    pub fn new(path: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            path: path.into(),
            bytes: bytes.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtualPackage {
    manifest: Vec<u8>,
    payloads: Vec<PackagePayload>,
}

impl VirtualPackage {
    pub fn builtin(
        draft: ModManifestDraft,
        payloads: Vec<PackagePayload>,
    ) -> Result<Self, PackageError> {
        let mut manifest = draft.with_hash(ContentHash::parse("0".repeat(64)).map_err(|_| {
            PackageError::InvalidManifest {
                field: "content_hash",
            }
        })?);
        let files = collect_payloads(payloads.clone(), PackageLimits::default())?;
        manifest.content_hash = canonical_payload_hash(&manifest, &files)?;
        let manifest = toml::to_string(&manifest)
            .map_err(|_| PackageError::ManifestCodec)?
            .into_bytes();
        Ok(Self { manifest, payloads })
    }

    #[must_use]
    pub fn from_raw(manifest: Vec<u8>, payloads: Vec<PackagePayload>) -> Self {
        Self { manifest, payloads }
    }

    #[must_use]
    pub fn manifest_bytes(&self) -> &[u8] {
        &self.manifest
    }

    #[must_use]
    pub fn payloads(&self) -> &[PackagePayload] {
        &self.payloads
    }
}

impl ModManifestDraft {
    fn with_hash(self, content_hash: ContentHash) -> ModManifest {
        ModManifest {
            schema_version: self.schema_version,
            mod_id: self.mod_id,
            version: self.version,
            pack_id: self.pack_id,
            engine: self.engine,
            content_schema: self.content_schema,
            dependencies: self.dependencies,
            capabilities: self.capabilities,
            patches: self.patches,
            prompts: self.prompts,
            content_hash,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageSource {
    Builtin(VirtualPackage),
    Directory(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackageLimits {
    pub max_files: usize,
    pub max_single_file_bytes: usize,
    pub max_total_bytes: usize,
    pub max_path_depth: usize,
    pub max_manifest_bytes: usize,
}

impl Default for PackageLimits {
    fn default() -> Self {
        Self {
            max_files: 256,
            max_single_file_bytes: 1_048_576,
            max_total_bytes: 16_777_216,
            max_path_depth: 8,
            max_manifest_bytes: 262_144,
        }
    }
}

impl PackageLimits {
    fn validate(self) -> Result<Self, PackageError> {
        let defaults = Self::default();
        if self.max_files == 0
            || self.max_single_file_bytes == 0
            || self.max_total_bytes == 0
            || self.max_path_depth == 0
            || self.max_manifest_bytes == 0
            || self.max_files > defaults.max_files
            || self.max_single_file_bytes > defaults.max_single_file_bytes
            || self.max_total_bytes > defaults.max_total_bytes
            || self.max_path_depth > defaults.max_path_depth
            || self.max_manifest_bytes > defaults.max_manifest_bytes
        {
            return Err(PackageError::InvalidLimits);
        }
        Ok(self)
    }
}

#[derive(Debug, Error)]
pub enum PackageError {
    #[error("package limits are invalid or expand the product defaults")]
    InvalidLimits,
    #[error("no Mod package source was configured")]
    NoPackages,
    #[error("package I/O failed during {stage}")]
    Io {
        stage: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("Mod manifest is not valid UTF-8 TOML")]
    ManifestCodec,
    #[error("Mod manifest field is invalid: {field}")]
    InvalidManifest { field: &'static str },
    #[error("package contains an unsafe or unsupported relative path")]
    UnsafePath,
    #[error("package contains a symbolic link")]
    Symlink,
    #[error("package exceeds resource limit {limit}")]
    ResourceLimit { limit: &'static str },
    #[error("package payload hash does not match for {mod_id}")]
    HashMismatch { mod_id: ModId },
    #[error("duplicate Mod package {mod_id}")]
    DuplicateMod { mod_id: ModId },
    #[error("required dependency is missing")]
    MissingDependency,
    #[error("installed dependency version is incompatible")]
    IncompatibleDependency,
    #[error("Mod dependency graph contains a cycle")]
    DependencyCycle,
    #[error("package JSON data is invalid")]
    InvalidData,
    #[error("definition is stored in the wrong package group")]
    InvalidDefinitionGroup,
    #[error("Patch declaration or target is invalid")]
    InvalidPatch,
    #[error("candidate ModLock does not match the save")]
    LockMismatch,
    #[error("candidate ModLock is invalid")]
    InvalidLock,
    #[error(transparent)]
    Content(#[from] ContentError),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackageResources {
    entries: BTreeMap<(ModId, String), Vec<u8>>,
}

impl PackageResources {
    #[must_use]
    pub fn get(&self, mod_id: &ModId, path: &str) -> Option<&[u8]> {
        self.entries
            .get(&(mod_id.clone(), path.to_owned()))
            .map(Vec::as_slice)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&ModId, &str, &[u8])> {
        self.entries
            .iter()
            .map(|((mod_id, path), bytes)| (mod_id, path.as_str(), bytes.as_slice()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledModSet {
    registry: DefinitionRegistry,
    mod_lock: ModLock,
    resources: PackageResources,
    prompts: CompiledAgentPrompts,
    prompt_sets: BTreeMap<ModId, CompiledAgentPrompts>,
    package_content: BTreeMap<ModId, PackageContentView>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompiledAgentPrompts {
    narrator: Vec<LongText>,
    npc: Vec<LongText>,
}

impl CompiledAgentPrompts {
    #[must_use]
    pub fn narrator(&self) -> &[LongText] {
        &self.narrator
    }

    #[must_use]
    pub fn npc(&self) -> &[LongText] {
        &self.npc
    }

    #[must_use]
    pub fn into_parts(self) -> (Vec<LongText>, Vec<LongText>) {
        (self.narrator, self.npc)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledWorldSet {
    registry: DefinitionRegistry,
    world_lock: WorldLock,
    mod_lock: ModLock,
    resources: PackageResources,
    prompts: CompiledAgentPrompts,
    package_content: BTreeMap<ModId, PackageContentView>,
}

impl CompiledWorldSet {
    #[must_use]
    pub fn registry(&self) -> &DefinitionRegistry {
        &self.registry
    }

    #[must_use]
    pub fn world_lock(&self) -> &WorldLock {
        &self.world_lock
    }

    #[must_use]
    pub fn mod_lock(&self) -> &ModLock {
        &self.mod_lock
    }

    #[must_use]
    pub fn resources(&self) -> &PackageResources {
        &self.resources
    }

    #[must_use]
    pub fn prompts(&self) -> &CompiledAgentPrompts {
        &self.prompts
    }

    #[must_use]
    pub fn package_content(&self) -> &BTreeMap<ModId, PackageContentView> {
        &self.package_content
    }

    #[must_use]
    pub fn into_parts(self) -> (DefinitionRegistry, WorldLock, ModLock, PackageResources) {
        (
            self.registry,
            self.world_lock,
            self.mod_lock,
            self.resources,
        )
    }
}

impl CompiledModSet {
    #[must_use]
    pub fn registry(&self) -> &DefinitionRegistry {
        &self.registry
    }

    #[must_use]
    pub fn mod_lock(&self) -> &ModLock {
        &self.mod_lock
    }

    #[must_use]
    pub fn resources(&self) -> &PackageResources {
        &self.resources
    }

    #[must_use]
    pub fn prompts(&self) -> &CompiledAgentPrompts {
        &self.prompts
    }

    #[must_use]
    pub fn package_content(&self) -> &BTreeMap<ModId, PackageContentView> {
        &self.package_content
    }

    #[must_use]
    pub fn into_parts(self) -> (DefinitionRegistry, ModLock, PackageResources) {
        (self.registry, self.mod_lock, self.resources)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageCompiler {
    engine_version: Version,
    limits: PackageLimits,
}

impl Default for PackageCompiler {
    fn default() -> Self {
        Self {
            engine_version: Version::new(0, 1, 0),
            limits: PackageLimits::default(),
        }
    }
}

impl PackageCompiler {
    pub fn new(engine_version: Version, limits: PackageLimits) -> Result<Self, PackageError> {
        Ok(Self {
            engine_version,
            limits: limits.validate()?,
        })
    }

    pub fn compile(
        &self,
        sources: impl IntoIterator<Item = PackageSource>,
    ) -> Result<CompiledModSet, PackageError> {
        self.compile_inner(sources, None)
    }

    pub fn compile_locked(
        &self,
        sources: impl IntoIterator<Item = PackageSource>,
        expected: &ModLock,
    ) -> Result<CompiledModSet, PackageError> {
        self.compile_inner(sources, Some(expected))
    }

    /// Inspects one installed directory package with the same safety, compatibility, and
    /// integrity checks used by the activation path, without resolving or enabling dependencies.
    pub fn inspect_directory(
        &self,
        root: impl AsRef<Path>,
    ) -> Result<InspectedPackage, PackageError> {
        self.limits.validate()?;
        let raw = read_directory_package(root.as_ref(), self.limits)?;
        let package = parse_package(raw, &self.engine_version, self.limits)?;
        let unit = parse_definition_documents(&package)?;
        let content = package_content_view(&package.manifest, &unit.documents);
        Ok(InspectedPackage {
            manifest: package.manifest,
            content,
        })
    }

    pub fn compile_world(
        &self,
        world: &WorldProjectSource,
        engine_sources: impl IntoIterator<Item = PackageSource>,
        mod_sources: impl IntoIterator<Item = PackageSource>,
        engine_namespaces: &BTreeSet<ModId>,
    ) -> Result<CompiledWorldSet, WorldProjectError> {
        let mut sources = engine_sources.into_iter().collect::<Vec<_>>();
        sources.push(PackageSource::Builtin(world.package().clone()));
        sources.extend(mod_sources);
        let CompiledModSet {
            registry,
            mod_lock: full_lock,
            resources,
            prompts: _,
            prompt_sets,
            package_content,
        } = self.compile_inner(sources, None)?;
        let (world_lock, mod_lock) = world.split_lock(full_lock, engine_namespaces)?;
        let prompt_order = std::iter::once(&world.manifest().world_id)
            .chain(mod_lock.mods.iter().map(|locked| &locked.mod_id));
        let prompts = flatten_prompt_sets(prompt_order, &prompt_sets);
        Ok(CompiledWorldSet {
            registry,
            world_lock,
            mod_lock,
            resources,
            prompts,
            package_content,
        })
    }

    fn compile_inner(
        &self,
        sources: impl IntoIterator<Item = PackageSource>,
        expected: Option<&ModLock>,
    ) -> Result<CompiledModSet, PackageError> {
        self.limits.validate()?;
        let mut parsed = BTreeMap::new();
        for source in sources {
            let raw = match source {
                PackageSource::Builtin(package) => RawPackage {
                    source_kind: ModSourceKind::Builtin,
                    manifest: package.manifest,
                    payloads: package.payloads,
                },
                PackageSource::Directory(root) => read_directory_package(&root, self.limits)?,
            };
            let package = parse_package(raw, &self.engine_version, self.limits)?;
            let mod_id = package.manifest.mod_id.clone();
            if parsed.insert(mod_id.clone(), package).is_some() {
                return Err(PackageError::DuplicateMod { mod_id });
            }
        }
        if parsed.is_empty() {
            return Err(PackageError::NoPackages);
        }
        let order = dependency_order(&parsed)?;
        let mut units = BTreeMap::new();
        for mod_id in &order {
            let package = parsed.get(mod_id).ok_or(PackageError::InvalidManifest {
                field: "dependency_graph",
            })?;
            units.insert(mod_id.clone(), parse_definition_documents(package)?);
        }
        let mut package_content = BTreeMap::new();
        for mod_id in &order {
            let package = parsed.get(mod_id).ok_or(PackageError::InvalidManifest {
                field: "dependency_graph",
            })?;
            let unit = units.get(mod_id).ok_or(PackageError::InvalidManifest {
                field: "dependency_graph",
            })?;
            package_content.insert(
                mod_id.clone(),
                package_content_view(&package.manifest, &unit.documents),
            );
        }
        let baseline = build_registry(&order, &units)?;
        apply_patches(&order, &parsed, &baseline, &mut units)?;
        let registry = build_registry(&order, &units)?;
        let mod_lock = build_lock(&order, &parsed)?;
        if expected.is_some_and(|expected| expected != &mod_lock) {
            return Err(PackageError::LockMismatch);
        }
        let resources = collect_resources(&order, &parsed);
        let prompt_sets = collect_prompt_sets(&order, &parsed)?;
        let prompts = flatten_prompt_sets(&order, &prompt_sets);
        Ok(CompiledModSet {
            registry,
            mod_lock,
            resources,
            prompts,
            prompt_sets,
            package_content,
        })
    }
}

#[derive(Debug)]
struct RawPackage {
    source_kind: ModSourceKind,
    manifest: Vec<u8>,
    payloads: Vec<PackagePayload>,
}

#[derive(Debug)]
struct ParsedPackage {
    source_kind: ModSourceKind,
    manifest: ModManifest,
    files: BTreeMap<String, Vec<u8>>,
}

#[derive(Debug, Clone)]
struct PackageUnit {
    context: ContentPackContext,
    documents: Vec<ContentDocument>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PatchDocument {
    schema_version: u32,
    operations: Vec<PatchOperation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum PatchOperation {
    ReplaceDefinition { value: Box<Definition> },
}

fn parse_package(
    raw: RawPackage,
    engine_version: &Version,
    limits: PackageLimits,
) -> Result<ParsedPackage, PackageError> {
    if raw.manifest.len() > limits.max_manifest_bytes {
        return Err(PackageError::ResourceLimit {
            limit: "manifest_bytes",
        });
    }
    let manifest_text =
        std::str::from_utf8(&raw.manifest).map_err(|_| PackageError::ManifestCodec)?;
    let manifest: ModManifest =
        toml::from_str(manifest_text).map_err(|_| PackageError::ManifestCodec)?;
    validate_manifest(&manifest, engine_version, limits)?;
    let files = collect_payloads(raw.payloads, limits)?;
    validate_payload_groups(&manifest, &files)?;
    if canonical_payload_hash(&manifest, &files)? != manifest.content_hash {
        return Err(PackageError::HashMismatch {
            mod_id: manifest.mod_id,
        });
    }
    Ok(ParsedPackage {
        source_kind: raw.source_kind,
        manifest,
        files,
    })
}

fn validate_manifest(
    manifest: &ModManifest,
    engine_version: &Version,
    limits: PackageLimits,
) -> Result<(), PackageError> {
    if manifest.schema_version != MOD_MANIFEST_SCHEMA_V1 {
        return invalid_manifest("schema_version");
    }
    if manifest.content_schema != CONTENT_SCHEMA_V1 {
        return invalid_manifest("content_schema");
    }
    if !manifest.engine.matches(engine_version) {
        return invalid_manifest("engine");
    }
    if manifest.capabilities.is_empty() {
        return invalid_manifest("capabilities");
    }
    if manifest
        .pack_id
        .mod_id()
        .map_err(|_| PackageError::InvalidManifest { field: "pack_id" })?
        != manifest.mod_id
        || manifest
            .pack_id
            .kind()
            .map_err(|_| PackageError::InvalidManifest { field: "pack_id" })?
            != "pack"
    {
        return invalid_manifest("pack_id");
    }
    let mut capabilities = BTreeSet::new();
    if manifest
        .capabilities
        .iter()
        .any(|capability| !capabilities.insert(*capability))
    {
        return invalid_manifest("duplicate_capability");
    }
    let mut dependencies = BTreeSet::new();
    for dependency in &manifest.dependencies {
        if dependency.mod_id == manifest.mod_id || !dependencies.insert(dependency.mod_id.clone()) {
            return invalid_manifest("dependency");
        }
    }
    let mut patch_ids = BTreeSet::new();
    let mut patch_files = BTreeSet::new();
    for patch in &manifest.patches {
        validate_relative_path(&patch.file, limits.max_path_depth)?;
        if classify_path(&patch.file) != Some(PayloadKind::Patch)
            || patch.target_mod == manifest.mod_id
            || patch.id.mod_id().map_err(|_| PackageError::InvalidPatch)? != manifest.mod_id
            || patch.id.kind().map_err(|_| PackageError::InvalidPatch)? != "patch"
            || patch
                .target_definition
                .mod_id()
                .map_err(|_| PackageError::InvalidPatch)?
                != patch.target_mod
            || !patch_ids.insert(patch.id.clone())
            || !patch_files.insert(patch.file.clone())
        {
            return Err(PackageError::InvalidPatch);
        }
    }
    let mut prompt_paths = BTreeSet::new();
    for path in manifest
        .prompts
        .narrator
        .iter()
        .chain(&manifest.prompts.npc)
    {
        validate_relative_path(path, limits.max_path_depth)?;
        if classify_path(path) != Some(PayloadKind::Prompt) || !prompt_paths.insert(path.as_str()) {
            return invalid_manifest("prompts");
        }
    }
    Ok(())
}

fn collect_payloads(
    payloads: Vec<PackagePayload>,
    limits: PackageLimits,
) -> Result<BTreeMap<String, Vec<u8>>, PackageError> {
    if payloads.len() > limits.max_files {
        return Err(PackageError::ResourceLimit {
            limit: "file_count",
        });
    }
    let mut files = BTreeMap::new();
    let mut total = 0_usize;
    for payload in payloads {
        validate_relative_path(&payload.path, limits.max_path_depth)?;
        if payload.bytes.len() > limits.max_single_file_bytes {
            return Err(PackageError::ResourceLimit {
                limit: "single_file_bytes",
            });
        }
        total = total.saturating_add(payload.bytes.len());
        if total > limits.max_total_bytes {
            return Err(PackageError::ResourceLimit {
                limit: "total_bytes",
            });
        }
        if files.insert(payload.path, payload.bytes).is_some() {
            return Err(PackageError::UnsafePath);
        }
    }
    Ok(files)
}

fn validate_payload_groups(
    manifest: &ModManifest,
    files: &BTreeMap<String, Vec<u8>>,
) -> Result<(), PackageError> {
    let capabilities = manifest
        .capabilities
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let declared_patches = manifest
        .patches
        .iter()
        .map(|patch| patch.file.as_str())
        .collect::<BTreeSet<_>>();
    for (path, bytes) in files {
        match classify_path(path) {
            Some(PayloadKind::Content) if capabilities.contains(&ModCapability::Content) => {}
            Some(PayloadKind::Rules) if capabilities.contains(&ModCapability::Rules) => {}
            Some(PayloadKind::Patch) if declared_patches.contains(path.as_str()) => {}
            Some(PayloadKind::Locale) => {
                let value: serde_json::Value =
                    serde_json::from_slice(bytes).map_err(|_| PackageError::InvalidData)?;
                if !value.is_object() {
                    return Err(PackageError::InvalidData);
                }
            }
            Some(PayloadKind::Prompt) => {
                std::str::from_utf8(bytes).map_err(|_| PackageError::InvalidData)?;
            }
            Some(PayloadKind::Asset) => {}
            _ => return Err(PackageError::UnsafePath),
        }
    }
    if manifest
        .patches
        .iter()
        .any(|patch| !files.contains_key(&patch.file))
    {
        return Err(PackageError::InvalidPatch);
    }
    if manifest
        .prompts
        .narrator
        .iter()
        .chain(&manifest.prompts.npc)
        .any(|path| !files.contains_key(path))
    {
        return invalid_manifest("prompts");
    }
    Ok(())
}

fn parse_definition_documents(package: &ParsedPackage) -> Result<PackageUnit, PackageError> {
    let mut documents = Vec::new();
    for (path, bytes) in &package.files {
        let Some(kind @ (PayloadKind::Content | PayloadKind::Rules)) = classify_path(path) else {
            continue;
        };
        let document: ContentDocument =
            serde_json::from_slice(bytes).map_err(|_| PackageError::InvalidData)?;
        if document.schema_version != package.manifest.content_schema {
            return Err(PackageError::InvalidData);
        }
        if document.definitions.iter().any(|definition| {
            let rule_definition = is_rule_definition(definition);
            matches!(kind, PayloadKind::Content) == rule_definition
        }) {
            return Err(PackageError::InvalidDefinitionGroup);
        }
        documents.push(document);
    }
    Ok(PackageUnit {
        context: ContentPackContext {
            mod_id: package.manifest.mod_id.clone(),
            mod_version: package.manifest.version.clone(),
            pack_id: package.manifest.pack_id.clone(),
            content_version: package.manifest.content_schema,
            content_hash: package.manifest.content_hash.clone(),
        },
        documents,
    })
}

fn package_content_view(
    manifest: &ModManifest,
    documents: &[ContentDocument],
) -> PackageContentView {
    let mut content = PackageContentView {
        narrator_prompts: bounded_count(manifest.prompts.narrator.len()),
        npc_prompts: bounded_count(manifest.prompts.npc.len()),
        patches: bounded_count(manifest.patches.len()),
        ..PackageContentView::default()
    };
    for definition in documents.iter().flat_map(|document| &document.definitions) {
        let count = match definition {
            Definition::Character(_) => &mut content.characters,
            Definition::Scene(_) => &mut content.scenes,
            Definition::Place(_) => &mut content.places,
            Definition::Item(_) => &mut content.items,
            Definition::Skill(_) => &mut content.skills,
            Definition::Condition(_) => &mut content.conditions,
            Definition::Event(_) => &mut content.events,
            Definition::GameplayAction(_) => &mut content.gameplay_actions,
            Definition::Rule(_) => &mut content.rules,
            Definition::Parameter(_) => &mut content.parameters,
            Definition::AgentProfile(_)
            | Definition::GenerationPolicy(_)
            | Definition::Tag(_)
            | Definition::RelationshipKind(_)
            | Definition::Attribute(_)
            | Definition::Resource(_)
            | Definition::EquipmentSlot(_) => &mut content.support_definitions,
        };
        *count = count.saturating_add(1);
    }
    content
}

fn bounded_count(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}

fn is_rule_definition(definition: &Definition) -> bool {
    matches!(
        definition,
        Definition::Parameter(_)
            | Definition::Event(_)
            | Definition::GameplayAction(_)
            | Definition::Rule(_)
    )
}

fn build_registry(
    order: &[ModId],
    units: &BTreeMap<ModId, PackageUnit>,
) -> Result<DefinitionRegistry, PackageError> {
    let packages = order
        .iter()
        .map(|mod_id| {
            units
                .get(mod_id)
                .map(|unit| (unit.context.clone(), unit.documents.clone()))
                .ok_or(PackageError::InvalidManifest {
                    field: "dependency_graph",
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(DefinitionRegistry::build_packages(packages)?)
}

fn apply_patches(
    order: &[ModId],
    packages: &BTreeMap<ModId, ParsedPackage>,
    baseline: &DefinitionRegistry,
    units: &mut BTreeMap<ModId, PackageUnit>,
) -> Result<(), PackageError> {
    for mod_id in order {
        let package = packages.get(mod_id).ok_or(PackageError::InvalidPatch)?;
        let dependencies = package
            .manifest
            .dependencies
            .iter()
            .map(|dependency| &dependency.mod_id)
            .collect::<BTreeSet<_>>();
        let mut patches = package.manifest.patches.iter().collect::<Vec<_>>();
        patches.sort_by(|left, right| left.id.cmp(&right.id));
        for patch in patches {
            if !dependencies.contains(&patch.target_mod) {
                return Err(PackageError::InvalidPatch);
            }
            let target_package = packages
                .get(&patch.target_mod)
                .ok_or(PackageError::InvalidPatch)?;
            if !patch
                .target_version
                .matches(&target_package.manifest.version)
            {
                return Err(PackageError::InvalidPatch);
            }
            let target = baseline
                .get(&patch.target_definition)
                .ok_or(PackageError::InvalidPatch)?;
            if target.origin.mod_id != patch.target_mod {
                return Err(PackageError::InvalidPatch);
            }
            let document: PatchDocument = serde_json::from_slice(
                package
                    .files
                    .get(&patch.file)
                    .ok_or(PackageError::InvalidPatch)?,
            )
            .map_err(|_| PackageError::InvalidData)?;
            if document.schema_version != CONTENT_SCHEMA_V1 || document.operations.is_empty() {
                return Err(PackageError::InvalidPatch);
            }
            for operation in document.operations {
                match operation {
                    PatchOperation::ReplaceDefinition { value } => {
                        replace_definition(units, &patch.target_definition, *value)?
                    }
                }
            }
        }
    }
    Ok(())
}

fn replace_definition(
    units: &mut BTreeMap<ModId, PackageUnit>,
    target_id: &ContentDefinitionId,
    replacement: Definition,
) -> Result<(), PackageError> {
    if replacement.id() != target_id {
        return Err(PackageError::InvalidPatch);
    }
    for unit in units.values_mut() {
        for document in &mut unit.documents {
            if let Some(target) = document
                .definitions
                .iter_mut()
                .find(|definition| definition.id() == target_id)
            {
                if target.expected_kind() != replacement.expected_kind() {
                    return Err(PackageError::InvalidPatch);
                }
                *target = replacement;
                return Ok(());
            }
        }
    }
    Err(PackageError::InvalidPatch)
}

fn dependency_order(packages: &BTreeMap<ModId, ParsedPackage>) -> Result<Vec<ModId>, PackageError> {
    let mut incoming = packages
        .keys()
        .cloned()
        .map(|mod_id| (mod_id, 0_usize))
        .collect::<BTreeMap<_, _>>();
    let mut outgoing = BTreeMap::<ModId, Vec<ModId>>::new();
    for (mod_id, package) in packages {
        for dependency in &package.manifest.dependencies {
            let Some(target) = packages.get(&dependency.mod_id) else {
                if dependency.optional {
                    continue;
                }
                return Err(PackageError::MissingDependency);
            };
            if !dependency.requirement.matches(&target.manifest.version) {
                return Err(PackageError::IncompatibleDependency);
            }
            let Some(count) = incoming.get_mut(mod_id) else {
                return invalid_manifest("dependency_graph");
            };
            *count += 1;
            outgoing
                .entry(dependency.mod_id.clone())
                .or_default()
                .push(mod_id.clone());
        }
    }
    for dependents in outgoing.values_mut() {
        dependents.sort();
    }
    let mut ready = incoming
        .iter()
        .filter(|(_, count)| **count == 0)
        .map(|(mod_id, _)| mod_id.clone())
        .collect::<BTreeSet<_>>();
    let mut order = Vec::with_capacity(packages.len());
    while let Some(mod_id) = ready.pop_first() {
        order.push(mod_id.clone());
        if let Some(dependents) = outgoing.get(&mod_id) {
            for dependent in dependents {
                let Some(count) = incoming.get_mut(dependent) else {
                    return invalid_manifest("dependency_graph");
                };
                *count = count.saturating_sub(1);
                if *count == 0 {
                    ready.insert(dependent.clone());
                }
            }
        }
    }
    if order.len() != packages.len() {
        return Err(PackageError::DependencyCycle);
    }
    Ok(order)
}

fn build_lock(
    order: &[ModId],
    packages: &BTreeMap<ModId, ParsedPackage>,
) -> Result<ModLock, PackageError> {
    let mut mods = Vec::with_capacity(order.len());
    for mod_id in order {
        let package = packages.get(mod_id).ok_or(PackageError::InvalidLock)?;
        let mut dependencies = package
            .manifest
            .dependencies
            .iter()
            .filter_map(|dependency| {
                packages
                    .get(&dependency.mod_id)
                    .map(|target| LockedDependency {
                        mod_id: dependency.mod_id.clone(),
                        version: target.manifest.version.clone(),
                        optional: dependency.optional,
                    })
            })
            .collect::<Vec<_>>();
        dependencies.sort_by(|left, right| left.mod_id.cmp(&right.mod_id));
        let mut applied_patches = package
            .manifest
            .patches
            .iter()
            .map(|patch| patch.id.clone())
            .collect::<Vec<_>>();
        applied_patches.sort();
        mods.push(LockedMod {
            mod_id: mod_id.clone(),
            version: package.manifest.version.clone(),
            content_hash: package.manifest.content_hash.clone(),
            manifest_schema: package.manifest.schema_version,
            content_schema: package.manifest.content_schema,
            source_kind: package.source_kind,
            dependencies,
            applied_patches,
        });
    }
    let lock = ModLock { mods };
    lock.validate().map_err(|_| PackageError::InvalidLock)?;
    Ok(lock)
}

fn collect_resources(
    order: &[ModId],
    packages: &BTreeMap<ModId, ParsedPackage>,
) -> PackageResources {
    let mut entries = BTreeMap::new();
    for mod_id in order {
        if let Some(package) = packages.get(mod_id) {
            for (path, bytes) in &package.files {
                if matches!(
                    classify_path(path),
                    Some(PayloadKind::Locale | PayloadKind::Prompt | PayloadKind::Asset)
                ) {
                    entries.insert((mod_id.clone(), path.clone()), bytes.clone());
                }
            }
        }
    }
    PackageResources { entries }
}

fn collect_prompt_sets(
    order: &[ModId],
    packages: &BTreeMap<ModId, ParsedPackage>,
) -> Result<BTreeMap<ModId, CompiledAgentPrompts>, PackageError> {
    let mut sets = BTreeMap::new();
    for mod_id in order {
        let package = packages.get(mod_id).ok_or(PackageError::InvalidManifest {
            field: "dependency_graph",
        })?;
        let compile = |paths: &[String]| -> Result<Vec<LongText>, PackageError> {
            paths
                .iter()
                .map(|path| {
                    let bytes = package
                        .files
                        .get(path)
                        .ok_or(PackageError::InvalidManifest { field: "prompts" })?;
                    let text = std::str::from_utf8(bytes).map_err(|_| PackageError::InvalidData)?;
                    LongText::non_empty(text).map_err(|_| PackageError::InvalidData)
                })
                .collect()
        };
        sets.insert(
            mod_id.clone(),
            CompiledAgentPrompts {
                narrator: compile(&package.manifest.prompts.narrator)?,
                npc: compile(&package.manifest.prompts.npc)?,
            },
        );
    }
    Ok(sets)
}

fn flatten_prompt_sets<'a>(
    order: impl IntoIterator<Item = &'a ModId>,
    sets: &BTreeMap<ModId, CompiledAgentPrompts>,
) -> CompiledAgentPrompts {
    let mut prompts = CompiledAgentPrompts::default();
    for mod_id in order {
        if let Some(set) = sets.get(mod_id) {
            prompts.narrator.extend(set.narrator.iter().cloned());
            prompts.npc.extend(set.npc.iter().cloned());
        }
    }
    prompts
}

fn canonical_payload_hash(
    manifest: &ModManifest,
    files: &BTreeMap<String, Vec<u8>>,
) -> Result<ContentHash, PackageError> {
    let manifest_bytes = canonical_manifest_bytes(manifest)?;
    let mut digest = Sha256::new();
    digest.update(length_bytes(manifest_bytes.len())?);
    digest.update(&manifest_bytes);
    for (path, bytes) in files {
        digest.update(length_bytes(path.len())?);
        digest.update(path.as_bytes());
        digest.update(length_bytes(bytes.len())?);
        digest.update(bytes);
    }
    ContentHash::parse(format!("{:x}", digest.finalize())).map_err(|_| {
        PackageError::InvalidManifest {
            field: "content_hash",
        }
    })
}

#[derive(Serialize)]
struct CanonicalManifest<'a> {
    schema_version: u32,
    mod_id: &'a ModId,
    version: &'a Version,
    pack_id: &'a ContentDefinitionId,
    engine: &'a VersionReq,
    content_schema: u32,
    dependencies: Vec<&'a ModDependency>,
    capabilities: Vec<ModCapability>,
    patches: Vec<&'a PatchDeclaration>,
    prompts: &'a PromptManifest,
    content_hash: &'static str,
}

fn canonical_manifest_bytes(manifest: &ModManifest) -> Result<Vec<u8>, PackageError> {
    let mut dependencies = manifest.dependencies.iter().collect::<Vec<_>>();
    dependencies.sort_by(|left, right| left.mod_id.cmp(&right.mod_id));
    let mut capabilities = manifest.capabilities.clone();
    capabilities.sort();
    let mut patches = manifest.patches.iter().collect::<Vec<_>>();
    patches.sort_by(|left, right| left.id.cmp(&right.id));
    toml::to_string(&CanonicalManifest {
        schema_version: manifest.schema_version,
        mod_id: &manifest.mod_id,
        version: &manifest.version,
        pack_id: &manifest.pack_id,
        engine: &manifest.engine,
        content_schema: manifest.content_schema,
        dependencies,
        capabilities,
        patches,
        prompts: &manifest.prompts,
        content_hash: "",
    })
    .map(String::into_bytes)
    .map_err(|_| PackageError::ManifestCodec)
}

fn length_bytes(length: usize) -> Result<[u8; 8], PackageError> {
    u64::try_from(length)
        .map(u64::to_le_bytes)
        .map_err(|_| PackageError::ResourceLimit {
            limit: "address_space",
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PayloadKind {
    Content,
    Rules,
    Patch,
    Locale,
    Prompt,
    Asset,
}

fn classify_path(path: &str) -> Option<PayloadKind> {
    let segments = path.split('/').collect::<Vec<_>>();
    match segments.as_slice() {
        ["content", file] if file.ends_with(".json") => Some(PayloadKind::Content),
        ["rules", file] if file.ends_with(".json") => Some(PayloadKind::Rules),
        ["patches", file] if file.ends_with(".json") => Some(PayloadKind::Patch),
        ["locales", file] if file.ends_with(".json") => Some(PayloadKind::Locale),
        ["prompts", file] if file.ends_with(".md") => Some(PayloadKind::Prompt),
        ["assets", _, ..] => Some(PayloadKind::Asset),
        _ => None,
    }
}

fn validate_relative_path(path: &str, max_depth: usize) -> Result<(), PackageError> {
    if path.is_empty()
        || path.starts_with('/')
        || path.starts_with('\\')
        || path.contains('\\')
        || path.contains('\0')
    {
        return Err(PackageError::UnsafePath);
    }
    let segments = path.split('/').collect::<Vec<_>>();
    if segments.len() > max_depth {
        return Err(PackageError::ResourceLimit {
            limit: "path_depth",
        });
    }
    if segments
        .iter()
        .any(|segment| segment.is_empty() || *segment == "." || *segment == "..")
    {
        return Err(PackageError::UnsafePath);
    }
    Ok(())
}

fn read_directory_package(root: &Path, limits: PackageLimits) -> Result<RawPackage, PackageError> {
    let root_metadata = fs::symlink_metadata(root).map_err(|source| PackageError::Io {
        stage: "inspect package root",
        source,
    })?;
    if root_metadata.file_type().is_symlink() {
        return Err(PackageError::Symlink);
    }
    if !root_metadata.is_dir() {
        return Err(PackageError::UnsafePath);
    }
    let manifest_path = root.join("mod.toml");
    let manifest_metadata =
        fs::symlink_metadata(&manifest_path).map_err(|source| PackageError::Io {
            stage: "inspect manifest",
            source,
        })?;
    if manifest_metadata.file_type().is_symlink() {
        return Err(PackageError::Symlink);
    }
    if !manifest_metadata.is_file() {
        return Err(PackageError::UnsafePath);
    }
    if manifest_metadata.len()
        > u64::try_from(limits.max_manifest_bytes).map_err(|_| PackageError::ResourceLimit {
            limit: "address_space",
        })?
    {
        return Err(PackageError::ResourceLimit {
            limit: "manifest_bytes",
        });
    }
    let manifest = fs::read(&manifest_path).map_err(|source| PackageError::Io {
        stage: "read manifest",
        source,
    })?;
    if manifest.len() > limits.max_manifest_bytes {
        return Err(PackageError::ResourceLimit {
            limit: "manifest_bytes",
        });
    }
    let mut payloads = Vec::new();
    let mut entries_seen = 0_usize;
    let mut total_bytes = 0_usize;
    read_payload_directory(
        root,
        root,
        limits,
        &mut entries_seen,
        &mut total_bytes,
        &mut payloads,
    )?;
    Ok(RawPackage {
        source_kind: ModSourceKind::Directory,
        manifest,
        payloads,
    })
}

fn read_payload_directory(
    root: &Path,
    directory: &Path,
    limits: PackageLimits,
    entries_seen: &mut usize,
    total_bytes: &mut usize,
    payloads: &mut Vec<PackagePayload>,
) -> Result<(), PackageError> {
    let read = fs::read_dir(directory).map_err(|source| PackageError::Io {
        stage: "read package directory",
        source,
    })?;
    let mut entries = Vec::new();
    for entry in read {
        let entry = entry.map_err(|source| PackageError::Io {
            stage: "read package entry",
            source,
        })?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| PackageError::UnsafePath)?;
        entries.push((name, entry.path()));
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    for (name, path) in entries {
        if directory == root && name == ".gitignore" {
            continue;
        }
        *entries_seen = entries_seen.saturating_add(1);
        if *entries_seen > limits.max_files.saturating_mul(2) {
            return Err(PackageError::ResourceLimit {
                limit: "directory_entries",
            });
        }
        let relative = relative_path(root, &path)?;
        if relative == "mod.toml" {
            continue;
        }
        validate_relative_path(&relative, limits.max_path_depth)?;
        let metadata = fs::symlink_metadata(&path).map_err(|source| PackageError::Io {
            stage: "inspect package entry",
            source,
        })?;
        if metadata.file_type().is_symlink() {
            return Err(PackageError::Symlink);
        }
        if metadata.is_dir() {
            read_payload_directory(root, &path, limits, entries_seen, total_bytes, payloads)?;
        } else if metadata.is_file() {
            let file_bytes =
                usize::try_from(metadata.len()).map_err(|_| PackageError::ResourceLimit {
                    limit: "address_space",
                })?;
            if file_bytes > limits.max_single_file_bytes {
                return Err(PackageError::ResourceLimit {
                    limit: "single_file_bytes",
                });
            }
            *total_bytes = total_bytes.saturating_add(file_bytes);
            if *total_bytes > limits.max_total_bytes {
                return Err(PackageError::ResourceLimit {
                    limit: "total_bytes",
                });
            }
            let bytes = fs::read(&path).map_err(|source| PackageError::Io {
                stage: "read package payload",
                source,
            })?;
            payloads.push(PackagePayload::new(relative, bytes));
            if payloads.len() > limits.max_files {
                return Err(PackageError::ResourceLimit {
                    limit: "file_count",
                });
            }
        } else {
            return Err(PackageError::UnsafePath);
        }
    }
    Ok(())
}

fn relative_path(root: &Path, path: &Path) -> Result<String, PackageError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| PackageError::UnsafePath)?;
    let mut segments = Vec::new();
    for component in relative.components() {
        let Component::Normal(segment) = component else {
            return Err(PackageError::UnsafePath);
        };
        segments.push(segment.to_str().ok_or(PackageError::UnsafePath)?);
    }
    Ok(segments.join("/"))
}

fn invalid_manifest<T>(field: &'static str) -> Result<T, PackageError> {
    Err(PackageError::InvalidManifest { field })
}
