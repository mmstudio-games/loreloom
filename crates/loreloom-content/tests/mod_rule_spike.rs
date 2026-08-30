//! P0 evidence for bounded data Mod packages and declarative rules.
//!
//! The schemas in this file are test-only. The spike validates the package,
//! dependency, patch, parameter, event, gameplay-action, and rule boundaries
//! before Loreloom exposes public content APIs.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const ENGINE_VERSION: &str = "0.1.0";
const CONTENT_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PackageSource {
    Builtin,
    Directory,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct VirtualFile {
    path: String,
    bytes: Vec<u8>,
    symlink: bool,
}

impl VirtualFile {
    fn json(path: &str, value: &impl Serialize) -> Self {
        Self {
            path: path.to_owned(),
            bytes: serde_json::to_vec(value).expect("fixture JSON serializes"),
            symlink: false,
        }
    }

    fn bytes(path: &str, bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            path: path.to_owned(),
            bytes: bytes.into(),
            symlink: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ModCapability {
    Content,
    Rules,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DependencyDeclaration {
    id: String,
    requirement: VersionReq,
    #[serde(default)]
    optional: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PatchDeclaration {
    id: String,
    file: String,
    target_mod: String,
    target_version: VersionReq,
    target_definition: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModManifest {
    schema_version: u32,
    id: String,
    version: Version,
    engine: VersionReq,
    content_schema: u32,
    #[serde(default)]
    dependencies: Vec<DependencyDeclaration>,
    #[serde(default)]
    capabilities: Vec<ModCapability>,
    #[serde(default)]
    patches: Vec<PatchDeclaration>,
    content_hash: String,
}

#[derive(Clone, Debug)]
struct VirtualPackage {
    source: PackageSource,
    manifest: Vec<u8>,
    files: Vec<VirtualFile>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PackageLimits {
    max_files: usize,
    max_single_file_bytes: usize,
    max_total_bytes: usize,
    max_path_depth: usize,
    max_manifest_bytes: usize,
}

impl PackageLimits {
    const fn defaults() -> Self {
        Self {
            max_files: 256,
            max_single_file_bytes: 1_048_576,
            max_total_bytes: 16_777_216,
            max_path_depth: 8,
            max_manifest_bytes: 262_144,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ModError {
    Manifest(String),
    InvalidManifestField(String),
    UnsafePath(String),
    Symlink(String),
    ResourceLimit(&'static str),
    HashMismatch { mod_id: String },
    DuplicateMod(String),
    MissingDependency { mod_id: String, dependency: String },
    IncompatibleDependency { mod_id: String, dependency: String },
    DependencyCycle,
    Data(String),
    DuplicateDefinition(String),
    PatchTarget(String),
    LockMismatch,
    Parameter(String),
    StaleRevision { expected: u64, actual: u64 },
    EventOption(String),
    GameplayAction(String),
    Rule(String),
    RuleBudget(&'static str),
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.contains('.')
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b".-_".contains(&byte)
        })
}

fn validate_relative_path(path: &str, max_depth: usize) -> Result<(), ModError> {
    if path.is_empty()
        || path.starts_with('/')
        || path.starts_with('\\')
        || path.contains('\\')
        || path.contains('\0')
    {
        return Err(ModError::UnsafePath(path.to_owned()));
    }
    let segments = path.split('/').collect::<Vec<_>>();
    if segments.len() > max_depth {
        return Err(ModError::ResourceLimit("path_depth"));
    }
    if segments
        .iter()
        .any(|segment| segment.is_empty() || *segment == "." || *segment == "..")
    {
        return Err(ModError::UnsafePath(path.to_owned()));
    }
    Ok(())
}

fn canonical_hash(manifest: &ModManifest, files: &[VirtualFile]) -> String {
    let mut canonical_manifest = manifest.clone();
    canonical_manifest.content_hash.clear();
    canonical_manifest
        .dependencies
        .sort_by(|left, right| left.id.cmp(&right.id));
    canonical_manifest
        .capabilities
        .sort_by_key(|capability| match capability {
            ModCapability::Content => 0,
            ModCapability::Rules => 1,
        });
    canonical_manifest
        .patches
        .sort_by(|left, right| left.id.cmp(&right.id));
    let manifest_bytes = toml::to_string(&canonical_manifest)
        .expect("validated fixture manifest canonicalizes")
        .into_bytes();
    let mut sorted_files = files.iter().collect::<Vec<_>>();
    sorted_files.sort_by(|left, right| left.path.cmp(&right.path));

    let mut digest = Sha256::new();
    digest.update((manifest_bytes.len() as u64).to_le_bytes());
    digest.update(manifest_bytes);
    for file in sorted_files {
        digest.update((file.path.len() as u64).to_le_bytes());
        digest.update(file.path.as_bytes());
        digest.update((file.bytes.len() as u64).to_le_bytes());
        digest.update(&file.bytes);
    }
    format!("sha256:{:x}", digest.finalize())
}

fn package(
    source: PackageSource,
    mut manifest: ModManifest,
    files: Vec<VirtualFile>,
) -> VirtualPackage {
    manifest.content_hash = canonical_hash(&manifest, &files);
    VirtualPackage {
        source,
        manifest: toml::to_string(&manifest)
            .expect("fixture manifest serializes")
            .into_bytes(),
        files,
    }
}

#[derive(Clone, Debug)]
struct ParsedPackage {
    source: PackageSource,
    manifest: ModManifest,
    files: BTreeMap<String, Vec<u8>>,
}

fn parse_package(
    package: VirtualPackage,
    limits: PackageLimits,
) -> Result<ParsedPackage, ModError> {
    if package.manifest.len() > limits.max_manifest_bytes {
        return Err(ModError::ResourceLimit("manifest_bytes"));
    }
    if package.files.len() > limits.max_files {
        return Err(ModError::ResourceLimit("file_count"));
    }
    let manifest_text = std::str::from_utf8(&package.manifest)
        .map_err(|error| ModError::Manifest(error.to_string()))?;
    let manifest: ModManifest =
        toml::from_str(manifest_text).map_err(|error| ModError::Manifest(error.to_string()))?;
    if manifest.schema_version != 1 {
        return Err(ModError::InvalidManifestField("schema_version".to_owned()));
    }
    if !valid_id(&manifest.id) {
        return Err(ModError::InvalidManifestField("id".to_owned()));
    }
    let engine = Version::parse(ENGINE_VERSION).expect("engine version is valid");
    if !manifest.engine.matches(&engine) {
        return Err(ModError::InvalidManifestField("engine".to_owned()));
    }
    if manifest.content_schema != CONTENT_SCHEMA_VERSION {
        return Err(ModError::InvalidManifestField("content_schema".to_owned()));
    }
    if manifest.capabilities.is_empty() {
        return Err(ModError::InvalidManifestField("capabilities".to_owned()));
    }

    let mut total_bytes = 0_usize;
    let mut files = BTreeMap::new();
    for file in &package.files {
        validate_relative_path(&file.path, limits.max_path_depth)?;
        if file.symlink {
            return Err(ModError::Symlink(file.path.clone()));
        }
        if file.bytes.len() > limits.max_single_file_bytes {
            return Err(ModError::ResourceLimit("single_file_bytes"));
        }
        total_bytes = total_bytes.saturating_add(file.bytes.len());
        if total_bytes > limits.max_total_bytes {
            return Err(ModError::ResourceLimit("total_bytes"));
        }
        if files
            .insert(file.path.clone(), file.bytes.clone())
            .is_some()
        {
            return Err(ModError::UnsafePath(file.path.clone()));
        }
    }
    if canonical_hash(&manifest, &package.files) != manifest.content_hash {
        return Err(ModError::HashMismatch {
            mod_id: manifest.id.clone(),
        });
    }
    for patch in &manifest.patches {
        validate_relative_path(&patch.file, limits.max_path_depth)?;
        if !files.contains_key(&patch.file) {
            return Err(ModError::PatchTarget(patch.file.clone()));
        }
    }
    Ok(ParsedPackage {
        source: package.source,
        manifest,
        files,
    })
}

fn dependency_order(packages: &BTreeMap<String, ParsedPackage>) -> Result<Vec<String>, ModError> {
    let mut incoming = packages
        .keys()
        .map(|id| (id.clone(), 0_usize))
        .collect::<BTreeMap<_, _>>();
    let mut outgoing = BTreeMap::<String, Vec<String>>::new();
    for (mod_id, package) in packages {
        let mut seen = BTreeSet::new();
        for dependency in &package.manifest.dependencies {
            if !seen.insert(&dependency.id) {
                return Err(ModError::InvalidManifestField(format!(
                    "duplicate dependency {}",
                    dependency.id
                )));
            }
            let Some(target) = packages.get(&dependency.id) else {
                if dependency.optional {
                    continue;
                }
                return Err(ModError::MissingDependency {
                    mod_id: mod_id.clone(),
                    dependency: dependency.id.clone(),
                });
            };
            if !dependency.requirement.matches(&target.manifest.version) {
                return Err(ModError::IncompatibleDependency {
                    mod_id: mod_id.clone(),
                    dependency: dependency.id.clone(),
                });
            }
            *incoming
                .get_mut(mod_id)
                .expect("every package has an incoming counter") += 1;
            outgoing
                .entry(dependency.id.clone())
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
        .map(|(id, _)| id.clone())
        .collect::<BTreeSet<_>>();
    let mut ordered = Vec::with_capacity(packages.len());
    while let Some(id) = ready.pop_first() {
        ordered.push(id.clone());
        if let Some(dependents) = outgoing.get(&id) {
            for dependent in dependents {
                let count = incoming
                    .get_mut(dependent)
                    .expect("dependent has an incoming counter");
                *count -= 1;
                if *count == 0 {
                    ready.insert(dependent.clone());
                }
            }
        }
    }
    if ordered.len() != packages.len() {
        return Err(ModError::DependencyCycle);
    }
    Ok(ordered)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DisplayDefinition {
    id: String,
    display_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContentDocument {
    schema_version: u32,
    #[serde(default)]
    definitions: Vec<DisplayDefinition>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
enum PatchOperation {
    ReplaceDisplayName { value: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PatchDocument {
    schema_version: u32,
    operations: Vec<PatchOperation>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ParameterType {
    Bool,
    Fixed {
        scale: u32,
        minimum: i64,
        maximum: i64,
    },
    Counter {
        minimum: i64,
        maximum: i64,
    },
    Enum {
        variants: Vec<String>,
    },
    TagSet {
        allowed: Vec<String>,
        maximum: usize,
    },
    ObjectRef {
        allowed_kinds: Vec<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ParameterValue {
    Bool {
        value: bool,
    },
    Fixed {
        scale: u32,
        value: i64,
    },
    Counter {
        value: i64,
    },
    Enum {
        value: String,
    },
    TagSet {
        values: BTreeSet<String>,
    },
    ObjectRef {
        object_id: String,
        object_kind: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParameterDefinition {
    id: String,
    value_type: ParameterType,
    default: ParameterValue,
    persistent: bool,
}

fn validate_parameter_value(
    definition: &ParameterDefinition,
    value: &ParameterValue,
    objects: &BTreeMap<String, String>,
) -> Result<(), ModError> {
    let valid = match (&definition.value_type, value) {
        (ParameterType::Bool, ParameterValue::Bool { .. }) => true,
        (
            ParameterType::Fixed {
                scale,
                minimum,
                maximum,
            },
            ParameterValue::Fixed {
                scale: value_scale,
                value,
            },
        ) => scale == value_scale && value >= minimum && value <= maximum,
        (ParameterType::Counter { minimum, maximum }, ParameterValue::Counter { value }) => {
            value >= minimum && value <= maximum
        }
        (ParameterType::Enum { variants }, ParameterValue::Enum { value }) => {
            variants.contains(value)
        }
        (ParameterType::TagSet { allowed, maximum }, ParameterValue::TagSet { values }) => {
            values.len() <= *maximum && values.iter().all(|value| allowed.contains(value))
        }
        (
            ParameterType::ObjectRef { allowed_kinds },
            ParameterValue::ObjectRef {
                object_id,
                object_kind,
            },
        ) => {
            allowed_kinds.contains(object_kind)
                && objects
                    .get(object_id)
                    .is_some_and(|kind| kind == object_kind)
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(ModError::Parameter(definition.id.clone()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum PredicateDefinition {
    Always,
    CounterAtLeast { parameter_id: String, value: i64 },
    CounterBelow { parameter_id: String, value: i64 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum EffectDefinition {
    AdjustCounter {
        parameter_id: String,
        delta: i64,
    },
    SetParameter {
        parameter_id: String,
        value: ParameterValue,
    },
    EmitEvent {
        kind: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EventOptionDefinition {
    id: String,
    #[serde(default)]
    visible_if: Vec<PredicateDefinition>,
    #[serde(default)]
    enabled_if: Vec<PredicateDefinition>,
    effects: Vec<EffectDefinition>,
    next_node: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EventNodeDefinition {
    id: String,
    options: Vec<EventOptionDefinition>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EventDefinition {
    id: String,
    entry_node: String,
    nodes: Vec<EventNodeDefinition>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActionParameterDefinition {
    name: String,
    value_type: ParameterType,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GameplayActionDefinition {
    id: String,
    parameters: Vec<ActionParameterDefinition>,
    effects: Vec<EffectDefinition>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum TriggerDefinition {
    WorldEvent { kind: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleDefinition {
    id: String,
    priority: i32,
    trigger: TriggerDefinition,
    #[serde(default)]
    predicates: Vec<PredicateDefinition>,
    effects: Vec<EffectDefinition>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleDocument {
    schema_version: u32,
    #[serde(default)]
    parameters: Vec<ParameterDefinition>,
    #[serde(default)]
    events: Vec<EventDefinition>,
    #[serde(default)]
    gameplay_actions: Vec<GameplayActionDefinition>,
    #[serde(default)]
    rules: Vec<RuleDefinition>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RuleLimits {
    max_predicate_nodes_per_rule: usize,
    max_effects_per_rule: usize,
    max_triggered_rules: usize,
    max_evaluated_predicates: usize,
    max_applied_effects: usize,
    max_cascade_depth: usize,
}

impl RuleLimits {
    const fn defaults() -> Self {
        Self {
            max_predicate_nodes_per_rule: 64,
            max_effects_per_rule: 32,
            max_triggered_rules: 128,
            max_evaluated_predicates: 1_024,
            max_applied_effects: 512,
            max_cascade_depth: 8,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct OwnedDisplayDefinition {
    owner_mod: String,
    owner_version: Version,
    definition: DisplayDefinition,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct CompiledRegistry {
    displays: BTreeMap<String, OwnedDisplayDefinition>,
    parameters: BTreeMap<String, ParameterDefinition>,
    events: BTreeMap<String, EventDefinition>,
    gameplay_actions: BTreeMap<String, GameplayActionDefinition>,
    rules: BTreeMap<String, RuleDefinition>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct LockedDependency {
    id: String,
    version: Version,
    optional: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ModLockEntry {
    id: String,
    version: Version,
    content_hash: String,
    manifest_schema: u32,
    content_schema: u32,
    source_kind: String,
    dependencies: Vec<LockedDependency>,
    applied_patches: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct PublishedMods {
    registry: CompiledRegistry,
    lock: Vec<ModLockEntry>,
}

fn parse_json<T: for<'de> Deserialize<'de>>(bytes: &[u8], path: &str) -> Result<T, ModError> {
    serde_json::from_slice(bytes).map_err(|error| ModError::Data(format!("{path}: {error}")))
}

fn insert_unique<T>(map: &mut BTreeMap<String, T>, id: String, value: T) -> Result<(), ModError> {
    if map.insert(id.clone(), value).is_some() {
        Err(ModError::DuplicateDefinition(id))
    } else {
        Ok(())
    }
}

fn trigger_kind(trigger: &TriggerDefinition) -> &str {
    match trigger {
        TriggerDefinition::WorldEvent { kind } => kind,
    }
}

fn validate_rule_graph(registry: &CompiledRegistry, limits: RuleLimits) -> Result<(), ModError> {
    for rule in registry.rules.values() {
        if rule.predicates.len() > limits.max_predicate_nodes_per_rule {
            return Err(ModError::RuleBudget("predicate_nodes_per_rule"));
        }
        if rule.effects.len() > limits.max_effects_per_rule {
            return Err(ModError::RuleBudget("effects_per_rule"));
        }
    }
    let listeners =
        registry
            .rules
            .values()
            .fold(BTreeMap::<String, Vec<String>>::new(), |mut map, rule| {
                map.entry(trigger_kind(&rule.trigger).to_owned())
                    .or_default()
                    .push(rule.id.clone());
                map
            });
    let mut edges = BTreeMap::<String, BTreeSet<String>>::new();
    for rule in registry.rules.values() {
        for effect in &rule.effects {
            if let EffectDefinition::EmitEvent { kind } = effect
                && let Some(targets) = listeners.get(kind)
            {
                edges
                    .entry(rule.id.clone())
                    .or_default()
                    .extend(targets.iter().cloned());
            }
        }
    }
    fn visit(
        id: &str,
        edges: &BTreeMap<String, BTreeSet<String>>,
        visiting: &mut BTreeSet<String>,
        visited: &mut BTreeSet<String>,
    ) -> bool {
        if visiting.contains(id) {
            return false;
        }
        if visited.contains(id) {
            return true;
        }
        visiting.insert(id.to_owned());
        if edges.get(id).is_some_and(|targets| {
            targets
                .iter()
                .any(|target| !visit(target, edges, visiting, visited))
        }) {
            return false;
        }
        visiting.remove(id);
        visited.insert(id.to_owned());
        true
    }
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    if registry
        .rules
        .keys()
        .any(|id| !visit(id, &edges, &mut visiting, &mut visited))
    {
        return Err(ModError::Rule("static event cycle".to_owned()));
    }
    Ok(())
}

fn compile_packages(
    packages: &[VirtualPackage],
    package_limits: PackageLimits,
    rule_limits: RuleLimits,
) -> Result<PublishedMods, ModError> {
    let mut parsed = BTreeMap::new();
    for package in packages.iter().cloned() {
        let package = parse_package(package, package_limits)?;
        let id = package.manifest.id.clone();
        if parsed.insert(id.clone(), package).is_some() {
            return Err(ModError::DuplicateMod(id));
        }
    }
    let order = dependency_order(&parsed)?;
    let mut registry = CompiledRegistry::default();

    for mod_id in &order {
        let package = &parsed[mod_id];
        for (path, bytes) in &package.files {
            if path.starts_with("content/") && path.ends_with(".json") {
                let document: ContentDocument = parse_json(bytes, path)?;
                if document.schema_version != CONTENT_SCHEMA_VERSION {
                    return Err(ModError::Data(format!("{path}: schema")));
                }
                for definition in document.definitions {
                    insert_unique(
                        &mut registry.displays,
                        definition.id.clone(),
                        OwnedDisplayDefinition {
                            owner_mod: mod_id.clone(),
                            owner_version: package.manifest.version.clone(),
                            definition,
                        },
                    )?;
                }
            } else if path.starts_with("rules/") && path.ends_with(".json") {
                let document: RuleDocument = parse_json(bytes, path)?;
                if document.schema_version != CONTENT_SCHEMA_VERSION {
                    return Err(ModError::Data(format!("{path}: schema")));
                }
                for definition in document.parameters {
                    validate_parameter_value(&definition, &definition.default, &BTreeMap::new())?;
                    insert_unique(&mut registry.parameters, definition.id.clone(), definition)?;
                }
                for definition in document.events {
                    insert_unique(&mut registry.events, definition.id.clone(), definition)?;
                }
                for definition in document.gameplay_actions {
                    insert_unique(
                        &mut registry.gameplay_actions,
                        definition.id.clone(),
                        definition,
                    )?;
                }
                for definition in document.rules {
                    insert_unique(&mut registry.rules, definition.id.clone(), definition)?;
                }
            }
        }
    }

    for mod_id in &order {
        let package = &parsed[mod_id];
        let dependencies = package
            .manifest
            .dependencies
            .iter()
            .map(|dependency| dependency.id.as_str())
            .collect::<BTreeSet<_>>();
        let mut patches = package.manifest.patches.clone();
        patches.sort_by(|left, right| left.id.cmp(&right.id));
        for patch in patches {
            if !dependencies.contains(patch.target_mod.as_str()) {
                return Err(ModError::PatchTarget(patch.id));
            }
            let target = registry
                .displays
                .get_mut(&patch.target_definition)
                .ok_or_else(|| ModError::PatchTarget(patch.target_definition.clone()))?;
            if target.owner_mod != patch.target_mod
                || !patch.target_version.matches(&target.owner_version)
            {
                return Err(ModError::PatchTarget(patch.target_definition));
            }
            let document: PatchDocument = parse_json(
                package
                    .files
                    .get(&patch.file)
                    .ok_or_else(|| ModError::PatchTarget(patch.file.clone()))?,
                &patch.file,
            )?;
            if document.schema_version != CONTENT_SCHEMA_VERSION {
                return Err(ModError::PatchTarget(patch.file));
            }
            for operation in document.operations {
                match operation {
                    PatchOperation::ReplaceDisplayName { value } => {
                        target.definition.display_name = value;
                    }
                }
            }
        }
    }
    validate_rule_graph(&registry, rule_limits)?;

    let lock = order
        .iter()
        .map(|mod_id| {
            let package = &parsed[mod_id];
            let mut dependencies = package
                .manifest
                .dependencies
                .iter()
                .filter_map(|dependency| {
                    parsed.get(&dependency.id).map(|target| LockedDependency {
                        id: dependency.id.clone(),
                        version: target.manifest.version.clone(),
                        optional: dependency.optional,
                    })
                })
                .collect::<Vec<_>>();
            dependencies.sort_by(|left, right| left.id.cmp(&right.id));
            let mut applied_patches = package
                .manifest
                .patches
                .iter()
                .map(|patch| patch.id.clone())
                .collect::<Vec<_>>();
            applied_patches.sort();
            ModLockEntry {
                id: mod_id.clone(),
                version: package.manifest.version.clone(),
                content_hash: package.manifest.content_hash.clone(),
                manifest_schema: package.manifest.schema_version,
                content_schema: package.manifest.content_schema,
                source_kind: match package.source {
                    PackageSource::Builtin => "builtin",
                    PackageSource::Directory => "directory",
                }
                .to_owned(),
                dependencies,
                applied_patches,
            }
        })
        .collect();
    Ok(PublishedMods { registry, lock })
}

#[derive(Default)]
struct ModRuntime {
    published: PublishedMods,
}

impl ModRuntime {
    fn install(
        &mut self,
        packages: &[VirtualPackage],
        expected_lock: Option<&[ModLockEntry]>,
        package_limits: PackageLimits,
        rule_limits: RuleLimits,
    ) -> Result<(), ModError> {
        let candidate = compile_packages(packages, package_limits, rule_limits)?;
        if expected_lock.is_some_and(|expected| expected != candidate.lock) {
            return Err(ModError::LockMismatch);
        }
        self.published = candidate;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct EventInstance {
    id: String,
    definition_id: String,
    current_node: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RuleProvenance {
    principal: String,
    rule_id: String,
    source_event: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RuleWorld {
    revision: u64,
    parameters: BTreeMap<String, ParameterValue>,
    objects: BTreeMap<String, String>,
    events: BTreeMap<String, EventInstance>,
    effect_log: Vec<String>,
    provenance: Vec<RuleProvenance>,
}

fn counter(world: &RuleWorld, parameter_id: &str) -> Option<i64> {
    match world.parameters.get(parameter_id) {
        Some(ParameterValue::Counter { value }) => Some(*value),
        _ => None,
    }
}

fn predicate_matches(world: &RuleWorld, predicate: &PredicateDefinition) -> Result<bool, ModError> {
    match predicate {
        PredicateDefinition::Always => Ok(true),
        PredicateDefinition::CounterAtLeast {
            parameter_id,
            value,
        } => counter(world, parameter_id)
            .map(|current| current >= *value)
            .ok_or_else(|| ModError::Parameter(parameter_id.clone())),
        PredicateDefinition::CounterBelow {
            parameter_id,
            value,
        } => counter(world, parameter_id)
            .map(|current| current < *value)
            .ok_or_else(|| ModError::Parameter(parameter_id.clone())),
    }
}

fn apply_effect(
    registry: &CompiledRegistry,
    world: &mut RuleWorld,
    effect: &EffectDefinition,
) -> Result<Option<String>, ModError> {
    match effect {
        EffectDefinition::AdjustCounter {
            parameter_id,
            delta,
        } => {
            let definition = registry
                .parameters
                .get(parameter_id)
                .ok_or_else(|| ModError::Parameter(parameter_id.clone()))?;
            let current = counter(world, parameter_id)
                .ok_or_else(|| ModError::Parameter(parameter_id.clone()))?;
            let value = ParameterValue::Counter {
                value: current
                    .checked_add(*delta)
                    .ok_or_else(|| ModError::Parameter(parameter_id.clone()))?,
            };
            validate_parameter_value(definition, &value, &world.objects)?;
            world.parameters.insert(parameter_id.clone(), value);
            Ok(None)
        }
        EffectDefinition::SetParameter {
            parameter_id,
            value,
        } => {
            let definition = registry
                .parameters
                .get(parameter_id)
                .ok_or_else(|| ModError::Parameter(parameter_id.clone()))?;
            validate_parameter_value(definition, value, &world.objects)?;
            world.parameters.insert(parameter_id.clone(), value.clone());
            Ok(None)
        }
        EffectDefinition::EmitEvent { kind } => Ok(Some(kind.clone())),
    }
}

fn choose_event_option(
    registry: &CompiledRegistry,
    world: &mut RuleWorld,
    event_instance_id: &str,
    option_id: &str,
    expected_revision: u64,
) -> Result<(), ModError> {
    if expected_revision != world.revision {
        return Err(ModError::StaleRevision {
            expected: expected_revision,
            actual: world.revision,
        });
    }
    let instance = world
        .events
        .get(event_instance_id)
        .ok_or_else(|| ModError::EventOption(event_instance_id.to_owned()))?;
    let definition = registry
        .events
        .get(&instance.definition_id)
        .ok_or_else(|| ModError::EventOption(instance.definition_id.clone()))?;
    let node = definition
        .nodes
        .iter()
        .find(|node| node.id == instance.current_node)
        .ok_or_else(|| ModError::EventOption(instance.current_node.clone()))?;
    let option = node
        .options
        .iter()
        .find(|option| option.id == option_id)
        .ok_or_else(|| ModError::EventOption(option_id.to_owned()))?;
    if !option
        .visible_if
        .iter()
        .map(|predicate| predicate_matches(world, predicate))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .all(|matches| matches)
        || !option
            .enabled_if
            .iter()
            .map(|predicate| predicate_matches(world, predicate))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .all(|matches| matches)
    {
        return Err(ModError::EventOption(option_id.to_owned()));
    }
    let effects = option.effects.clone();
    let next_node = option.next_node.clone();
    let mut candidate = world.clone();
    for effect in &effects {
        if apply_effect(registry, &mut candidate, effect)?.is_some() {
            return Err(ModError::EventOption(
                "event option cannot emit recursive events directly".to_owned(),
            ));
        }
    }
    if let Some(next_node) = next_node {
        if !definition.nodes.iter().any(|node| node.id == next_node) {
            return Err(ModError::EventOption(next_node));
        }
        candidate
            .events
            .get_mut(event_instance_id)
            .expect("candidate retains event instance")
            .current_node = next_node;
    }
    candidate.revision += 1;
    *world = candidate;
    Ok(())
}

fn parameter_definition_for_action(
    action_id: &str,
    parameter: &ActionParameterDefinition,
) -> ParameterDefinition {
    let default = match &parameter.value_type {
        ParameterType::Bool => ParameterValue::Bool { value: false },
        ParameterType::Fixed { scale, minimum, .. } => ParameterValue::Fixed {
            scale: *scale,
            value: *minimum,
        },
        ParameterType::Counter { minimum, .. } => ParameterValue::Counter { value: *minimum },
        ParameterType::Enum { variants } => ParameterValue::Enum {
            value: variants.first().cloned().unwrap_or_default(),
        },
        ParameterType::TagSet { .. } => ParameterValue::TagSet {
            values: BTreeSet::new(),
        },
        ParameterType::ObjectRef { .. } => ParameterValue::ObjectRef {
            object_id: String::new(),
            object_kind: String::new(),
        },
    };
    ParameterDefinition {
        id: format!("{action_id}/{}", parameter.name),
        value_type: parameter.value_type.clone(),
        default,
        persistent: false,
    }
}

fn perform_gameplay_action(
    registry: &CompiledRegistry,
    world: &mut RuleWorld,
    action_id: &str,
    arguments: &BTreeMap<String, ParameterValue>,
) -> Result<(), ModError> {
    let action = registry
        .gameplay_actions
        .get(action_id)
        .ok_or_else(|| ModError::GameplayAction(action_id.to_owned()))?;
    if arguments.len() != action.parameters.len() {
        return Err(ModError::GameplayAction("argument cardinality".to_owned()));
    }
    for parameter in &action.parameters {
        let value = arguments
            .get(&parameter.name)
            .ok_or_else(|| ModError::GameplayAction(parameter.name.clone()))?;
        validate_parameter_value(
            &parameter_definition_for_action(action_id, parameter),
            value,
            &world.objects,
        )?;
    }
    let mut candidate = world.clone();
    for effect in &action.effects {
        if apply_effect(registry, &mut candidate, effect)?.is_some() {
            return Err(ModError::GameplayAction(
                "action cannot recurse into rules".to_owned(),
            ));
        }
    }
    candidate.revision += 1;
    *world = candidate;
    Ok(())
}

fn execute_rules(
    registry: &CompiledRegistry,
    world: &mut RuleWorld,
    initial_event: &str,
    limits: RuleLimits,
) -> Result<(), ModError> {
    let mut candidate = world.clone();
    let mut queue = VecDeque::from([(initial_event.to_owned(), 0_usize)]);
    let mut triggered = 0_usize;
    let mut predicates = 0_usize;
    let mut effects = 0_usize;
    while let Some((event, depth)) = queue.pop_front() {
        if depth > limits.max_cascade_depth {
            return Err(ModError::RuleBudget("cascade_depth"));
        }
        let mut matching = registry
            .rules
            .values()
            .filter(|rule| trigger_kind(&rule.trigger) == event)
            .collect::<Vec<_>>();
        matching.sort_by(|left, right| {
            left.priority
                .cmp(&right.priority)
                .then_with(|| left.id.cmp(&right.id))
        });
        for rule in matching {
            triggered += 1;
            if triggered > limits.max_triggered_rules {
                return Err(ModError::RuleBudget("triggered_rules"));
            }
            predicates = predicates.saturating_add(rule.predicates.len());
            if predicates > limits.max_evaluated_predicates {
                return Err(ModError::RuleBudget("evaluated_predicates"));
            }
            let matches = rule
                .predicates
                .iter()
                .map(|predicate| predicate_matches(&candidate, predicate))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .all(|matches| matches);
            if !matches {
                continue;
            }
            candidate.effect_log.push(rule.id.clone());
            candidate.provenance.push(RuleProvenance {
                principal: "system/rule".to_owned(),
                rule_id: rule.id.clone(),
                source_event: event.clone(),
            });
            for effect in &rule.effects {
                effects += 1;
                if effects > limits.max_applied_effects {
                    return Err(ModError::RuleBudget("applied_effects"));
                }
                if let Some(emitted) = apply_effect(registry, &mut candidate, effect)? {
                    queue.push_back((emitted, depth + 1));
                }
            }
        }
    }
    candidate.revision += 1;
    *world = candidate;
    Ok(())
}

fn manifest(id: &str, version: &str) -> ModManifest {
    ModManifest {
        schema_version: 1,
        id: id.to_owned(),
        version: Version::parse(version).expect("fixture version is valid"),
        engine: VersionReq::parse(">=0.1.0, <0.2.0").expect("fixture requirement is valid"),
        content_schema: CONTENT_SCHEMA_VERSION,
        dependencies: Vec::new(),
        capabilities: vec![ModCapability::Content, ModCapability::Rules],
        patches: Vec::new(),
        content_hash: String::new(),
    }
}

fn counter_parameter(id: &str, default: i64, maximum: i64) -> ParameterDefinition {
    ParameterDefinition {
        id: id.to_owned(),
        value_type: ParameterType::Counter {
            minimum: 0,
            maximum,
        },
        default: ParameterValue::Counter { value: default },
        persistent: true,
    }
}

fn base_rule_document() -> RuleDocument {
    RuleDocument {
        schema_version: CONTENT_SCHEMA_VERSION,
        parameters: vec![counter_parameter("base.parameter.alert", 0, 10)],
        events: vec![EventDefinition {
            id: "base.event.bell".to_owned(),
            entry_node: "silent".to_owned(),
            nodes: vec![
                EventNodeDefinition {
                    id: "silent".to_owned(),
                    options: vec![EventOptionDefinition {
                        id: "ring".to_owned(),
                        visible_if: vec![PredicateDefinition::Always],
                        enabled_if: vec![PredicateDefinition::CounterBelow {
                            parameter_id: "base.parameter.alert".to_owned(),
                            value: 10,
                        }],
                        effects: vec![EffectDefinition::AdjustCounter {
                            parameter_id: "base.parameter.alert".to_owned(),
                            delta: 1,
                        }],
                        next_node: Some("rung".to_owned()),
                    }],
                },
                EventNodeDefinition {
                    id: "rung".to_owned(),
                    options: Vec::new(),
                },
            ],
        }],
        gameplay_actions: vec![GameplayActionDefinition {
            id: "base.action.raise_alert".to_owned(),
            parameters: vec![ActionParameterDefinition {
                name: "amount".to_owned(),
                value_type: ParameterType::Counter {
                    minimum: 1,
                    maximum: 3,
                },
            }],
            effects: vec![EffectDefinition::AdjustCounter {
                parameter_id: "base.parameter.alert".to_owned(),
                delta: 2,
            }],
        }],
        rules: vec![
            RuleDefinition {
                id: "base.rule.first".to_owned(),
                priority: 10,
                trigger: TriggerDefinition::WorldEvent {
                    kind: "bell_rang".to_owned(),
                },
                predicates: vec![PredicateDefinition::Always],
                effects: vec![EffectDefinition::AdjustCounter {
                    parameter_id: "base.parameter.alert".to_owned(),
                    delta: 1,
                }],
            },
            RuleDefinition {
                id: "base.rule.second".to_owned(),
                priority: 20,
                trigger: TriggerDefinition::WorldEvent {
                    kind: "bell_rang".to_owned(),
                },
                predicates: vec![PredicateDefinition::CounterAtLeast {
                    parameter_id: "base.parameter.alert".to_owned(),
                    value: 1,
                }],
                effects: vec![EffectDefinition::AdjustCounter {
                    parameter_id: "base.parameter.alert".to_owned(),
                    delta: 1,
                }],
            },
        ],
    }
}

fn base_package(source: PackageSource) -> VirtualPackage {
    package(
        source,
        manifest("games.loreloom.base", "1.0.0"),
        vec![
            VirtualFile::json(
                "content/base.json",
                &ContentDocument {
                    schema_version: CONTENT_SCHEMA_VERSION,
                    definitions: vec![DisplayDefinition {
                        id: "base.character.mira".to_owned(),
                        display_name: "Mira".to_owned(),
                    }],
                },
            ),
            VirtualFile::json("rules/base.json", &base_rule_document()),
        ],
    )
}

fn world_from_registry(registry: &CompiledRegistry) -> RuleWorld {
    RuleWorld {
        revision: 0,
        parameters: registry
            .parameters
            .iter()
            .map(|(id, definition)| (id.clone(), definition.default.clone()))
            .collect(),
        objects: BTreeMap::from([("character/mira".to_owned(), "character".to_owned())]),
        events: BTreeMap::from([(
            "event/bell".to_owned(),
            EventInstance {
                id: "event/bell".to_owned(),
                definition_id: "base.event.bell".to_owned(),
                current_node: "silent".to_owned(),
            },
        )]),
        effect_log: Vec::new(),
        provenance: Vec::new(),
    }
}

#[test]
fn builtin_and_directory_packages_share_path_hash_and_resource_checks() {
    let limits = PackageLimits::defaults();
    let builtin = parse_package(base_package(PackageSource::Builtin), limits)
        .expect("builtin package validates");
    let directory = parse_package(base_package(PackageSource::Directory), limits)
        .expect("directory package validates through the same pipeline");
    assert_eq!(builtin.manifest.id, directory.manifest.id);
    assert_eq!(
        builtin.manifest.content_hash,
        directory.manifest.content_hash
    );
    assert_eq!(builtin.files, directory.files);

    for unsafe_path in ["../secret", "/absolute", "a\\b", "a/./b", "a//b", "a\0b"] {
        let mut package = base_package(PackageSource::Directory);
        package.files.push(VirtualFile::bytes(unsafe_path, b"x"));
        assert!(matches!(
            parse_package(package, limits),
            Err(ModError::UnsafePath(path)) if path == unsafe_path
        ));
    }
    let mut symlink = base_package(PackageSource::Directory);
    symlink.files.push(VirtualFile {
        path: "assets/link".to_owned(),
        bytes: Vec::new(),
        symlink: true,
    });
    assert!(matches!(
        parse_package(symlink, limits),
        Err(ModError::Symlink(path)) if path == "assets/link"
    ));

    let mut tampered = base_package(PackageSource::Directory);
    tampered.files[0].bytes.push(b' ');
    assert!(matches!(
        parse_package(tampered, limits),
        Err(ModError::HashMismatch { .. })
    ));

    let tiny = PackageLimits {
        max_files: 1,
        ..limits
    };
    assert_eq!(
        parse_package(base_package(PackageSource::Directory), tiny)
            .expect_err("file limit rejects package"),
        ModError::ResourceLimit("file_count")
    );
    assert_eq!(
        parse_package(
            base_package(PackageSource::Directory),
            PackageLimits {
                max_single_file_bytes: 1,
                ..limits
            },
        )
        .expect_err("single-file limit rejects package"),
        ModError::ResourceLimit("single_file_bytes")
    );
    assert_eq!(
        parse_package(
            base_package(PackageSource::Directory),
            PackageLimits {
                max_total_bytes: 1,
                ..limits
            },
        )
        .expect_err("total-byte limit rejects package"),
        ModError::ResourceLimit("total_bytes")
    );
    assert_eq!(
        parse_package(
            base_package(PackageSource::Directory),
            PackageLimits {
                max_path_depth: 1,
                ..limits
            },
        )
        .expect_err("path-depth limit rejects package"),
        ModError::ResourceLimit("path_depth")
    );
    assert_eq!(
        parse_package(
            base_package(PackageSource::Directory),
            PackageLimits {
                max_manifest_bytes: 1,
                ..limits
            },
        )
        .expect_err("manifest limit rejects package"),
        ModError::ResourceLimit("manifest_bytes")
    );
}

#[test]
fn dependencies_have_deterministic_semver_order_cycles_and_exact_lock() {
    let base = base_package(PackageSource::Builtin);
    let mut addon_manifest = manifest("games.example.addon", "1.2.0");
    addon_manifest.dependencies = vec![
        DependencyDeclaration {
            id: "games.loreloom.base".to_owned(),
            requirement: VersionReq::parse("^1.0").expect("fixture requirement is valid"),
            optional: false,
        },
        DependencyDeclaration {
            id: "games.example.optional".to_owned(),
            requirement: VersionReq::parse("^1.0").expect("fixture requirement is valid"),
            optional: true,
        },
    ];
    let addon = package(PackageSource::Directory, addon_manifest, Vec::new());
    let compiled = compile_packages(
        &[addon.clone(), base.clone()],
        PackageLimits::defaults(),
        RuleLimits::defaults(),
    )
    .expect("dependency graph compiles independent of input order");
    assert_eq!(
        compiled
            .lock
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        vec!["games.loreloom.base", "games.example.addon"]
    );
    assert_eq!(
        compiled.lock[1].dependencies[0].version,
        Version::new(1, 0, 0)
    );
    assert_eq!(compiled.lock[1].dependencies.len(), 1);

    let mut runtime = ModRuntime::default();
    runtime
        .install(
            &[base.clone(), addon.clone()],
            Some(&compiled.lock),
            PackageLimits::defaults(),
            RuleLimits::defaults(),
        )
        .expect("exact ModLock reopens");
    let before = runtime.published.clone();
    let mut changed_addon_manifest = manifest("games.example.addon", "1.3.0");
    changed_addon_manifest.dependencies = vec![DependencyDeclaration {
        id: "games.loreloom.base".to_owned(),
        requirement: VersionReq::parse("^1.0").expect("fixture requirement is valid"),
        optional: false,
    }];
    let changed_addon = package(PackageSource::Directory, changed_addon_manifest, Vec::new());
    assert_eq!(
        runtime.install(
            &[base.clone(), changed_addon],
            Some(&compiled.lock),
            PackageLimits::defaults(),
            RuleLimits::defaults(),
        ),
        Err(ModError::LockMismatch)
    );
    assert_eq!(runtime.published, before);

    let mut missing_manifest = manifest("games.example.missing", "1.0.0");
    missing_manifest.dependencies = vec![DependencyDeclaration {
        id: "games.not-installed".to_owned(),
        requirement: VersionReq::STAR,
        optional: false,
    }];
    let missing = package(PackageSource::Directory, missing_manifest, Vec::new());
    assert!(matches!(
        compile_packages(
            &[missing],
            PackageLimits::defaults(),
            RuleLimits::defaults()
        ),
        Err(ModError::MissingDependency { .. })
    ));

    let mut incompatible_manifest = manifest("games.example.incompatible", "1.0.0");
    incompatible_manifest.dependencies = vec![DependencyDeclaration {
        id: "games.loreloom.base".to_owned(),
        requirement: VersionReq::parse(">=2.0.0").expect("fixture requirement is valid"),
        optional: false,
    }];
    let incompatible = package(PackageSource::Directory, incompatible_manifest, Vec::new());
    assert!(matches!(
        compile_packages(
            &[base.clone(), incompatible],
            PackageLimits::defaults(),
            RuleLimits::defaults(),
        ),
        Err(ModError::IncompatibleDependency { .. })
    ));

    let mut first_manifest = manifest("games.cycle.first", "1.0.0");
    first_manifest.dependencies = vec![DependencyDeclaration {
        id: "games.cycle.second".to_owned(),
        requirement: VersionReq::STAR,
        optional: false,
    }];
    let mut second_manifest = manifest("games.cycle.second", "1.0.0");
    second_manifest.dependencies = vec![DependencyDeclaration {
        id: "games.cycle.first".to_owned(),
        requirement: VersionReq::STAR,
        optional: false,
    }];
    assert_eq!(
        compile_packages(
            &[
                package(PackageSource::Directory, first_manifest, Vec::new()),
                package(PackageSource::Directory, second_manifest, Vec::new()),
            ],
            PackageLimits::defaults(),
            RuleLimits::defaults(),
        ),
        Err(ModError::DependencyCycle)
    );
}

#[test]
fn duplicate_definitions_fail_and_explicit_versioned_patch_is_ordered() {
    let base = base_package(PackageSource::Builtin);
    let mut duplicate_manifest = manifest("games.example.duplicate", "1.0.0");
    duplicate_manifest.dependencies = vec![DependencyDeclaration {
        id: "games.loreloom.base".to_owned(),
        requirement: VersionReq::STAR,
        optional: false,
    }];
    let duplicate = package(
        PackageSource::Directory,
        duplicate_manifest,
        vec![VirtualFile::json(
            "content/duplicate.json",
            &ContentDocument {
                schema_version: CONTENT_SCHEMA_VERSION,
                definitions: vec![DisplayDefinition {
                    id: "base.character.mira".to_owned(),
                    display_name: "Impostor".to_owned(),
                }],
            },
        )],
    );
    assert!(matches!(
        compile_packages(
            &[base.clone(), duplicate],
            PackageLimits::defaults(),
            RuleLimits::defaults()
        ),
        Err(ModError::DuplicateDefinition(id)) if id == "base.character.mira"
    ));

    let patch_path = "patches/mira.json";
    let mut patch_manifest = manifest("games.example.names", "1.0.0");
    patch_manifest.dependencies = vec![DependencyDeclaration {
        id: "games.loreloom.base".to_owned(),
        requirement: VersionReq::parse("=1.0.0").expect("fixture requirement is valid"),
        optional: false,
    }];
    patch_manifest.patches = vec![PatchDeclaration {
        id: "games.example.names/patch/mira".to_owned(),
        file: patch_path.to_owned(),
        target_mod: "games.loreloom.base".to_owned(),
        target_version: VersionReq::parse("=1.0.0").expect("fixture requirement is valid"),
        target_definition: "base.character.mira".to_owned(),
    }];
    let patch = package(
        PackageSource::Directory,
        patch_manifest,
        vec![VirtualFile::json(
            patch_path,
            &PatchDocument {
                schema_version: CONTENT_SCHEMA_VERSION,
                operations: vec![PatchOperation::ReplaceDisplayName {
                    value: "Mira of the Mill".to_owned(),
                }],
            },
        )],
    );
    let compiled = compile_packages(
        &[patch, base],
        PackageLimits::defaults(),
        RuleLimits::defaults(),
    )
    .expect("explicit matching Patch applies");
    assert_eq!(
        compiled.registry.displays["base.character.mira"]
            .definition
            .display_name,
        "Mira of the Mill"
    );
    assert_eq!(
        compiled.lock[1].applied_patches,
        vec!["games.example.names/patch/mira"]
    );
}

#[test]
fn all_parameter_variants_are_typed_and_object_refs_are_world_checked() {
    let objects = BTreeMap::from([("character/mira".to_owned(), "character".to_owned())]);
    let definitions = vec![
        ParameterDefinition {
            id: "p.bool".to_owned(),
            value_type: ParameterType::Bool,
            default: ParameterValue::Bool { value: false },
            persistent: true,
        },
        ParameterDefinition {
            id: "p.fixed".to_owned(),
            value_type: ParameterType::Fixed {
                scale: 100,
                minimum: -500,
                maximum: 500,
            },
            default: ParameterValue::Fixed {
                scale: 100,
                value: 125,
            },
            persistent: true,
        },
        counter_parameter("p.counter", 2, 5),
        ParameterDefinition {
            id: "p.enum".to_owned(),
            value_type: ParameterType::Enum {
                variants: vec!["calm".to_owned(), "alert".to_owned()],
            },
            default: ParameterValue::Enum {
                value: "calm".to_owned(),
            },
            persistent: true,
        },
        ParameterDefinition {
            id: "p.tags".to_owned(),
            value_type: ParameterType::TagSet {
                allowed: vec!["rain".to_owned(), "night".to_owned()],
                maximum: 2,
            },
            default: ParameterValue::TagSet {
                values: BTreeSet::from(["night".to_owned()]),
            },
            persistent: true,
        },
        ParameterDefinition {
            id: "p.object".to_owned(),
            value_type: ParameterType::ObjectRef {
                allowed_kinds: vec!["character".to_owned()],
            },
            default: ParameterValue::ObjectRef {
                object_id: "character/mira".to_owned(),
                object_kind: "character".to_owned(),
            },
            persistent: true,
        },
    ];
    for definition in &definitions {
        validate_parameter_value(definition, &definition.default, &objects)
            .expect("typed default validates");
    }
    assert_eq!(
        validate_parameter_value(
            &definitions[1],
            &ParameterValue::Fixed {
                scale: 10,
                value: 125,
            },
            &objects,
        ),
        Err(ModError::Parameter("p.fixed".to_owned()))
    );
    assert_eq!(
        validate_parameter_value(
            &definitions[5],
            &ParameterValue::ObjectRef {
                object_id: "character/missing".to_owned(),
                object_kind: "character".to_owned(),
            },
            &objects,
        ),
        Err(ModError::Parameter("p.object".to_owned()))
    );
    let arbitrary_json = serde_json::from_value::<ParameterValue>(serde_json::json!({
        "untyped": { "anything": true }
    }));
    assert!(arbitrary_json.is_err());
}

#[test]
fn event_option_rechecks_revision_and_commits_effect_with_node_atomically() {
    let compiled = compile_packages(
        &[base_package(PackageSource::Builtin)],
        PackageLimits::defaults(),
        RuleLimits::defaults(),
    )
    .expect("base rules compile");
    let mut world = world_from_registry(&compiled.registry);
    world.revision = 4;
    let before = world.clone();
    assert_eq!(
        choose_event_option(&compiled.registry, &mut world, "event/bell", "ring", 3,),
        Err(ModError::StaleRevision {
            expected: 3,
            actual: 4,
        })
    );
    assert_eq!(world, before);

    choose_event_option(&compiled.registry, &mut world, "event/bell", "ring", 4)
        .expect("current option applies");
    assert_eq!(world.revision, 5);
    assert_eq!(counter(&world, "base.parameter.alert"), Some(1));
    assert_eq!(world.events["event/bell"].current_node, "rung");
}

#[test]
fn gameplay_action_uses_generic_entry_and_typed_effect_plan() {
    let compiled = compile_packages(
        &[base_package(PackageSource::Builtin)],
        PackageLimits::defaults(),
        RuleLimits::defaults(),
    )
    .expect("base actions compile");
    let generic_tool_names = ["list_gameplay_actions", "perform_gameplay_action"];
    assert!(!generic_tool_names.contains(&"base.action.raise_alert"));
    let mut world = world_from_registry(&compiled.registry);
    let before = world.clone();
    assert_eq!(
        perform_gameplay_action(
            &compiled.registry,
            &mut world,
            "base.action.raise_alert",
            &BTreeMap::from([("amount".to_owned(), ParameterValue::Counter { value: 9 },)]),
        ),
        Err(ModError::Parameter(
            "base.action.raise_alert/amount".to_owned()
        ))
    );
    assert_eq!(world, before);
    perform_gameplay_action(
        &compiled.registry,
        &mut world,
        "base.action.raise_alert",
        &BTreeMap::from([("amount".to_owned(), ParameterValue::Counter { value: 2 })]),
    )
    .expect("typed generic gameplay action applies compiled effects");
    assert_eq!(counter(&world, "base.parameter.alert"), Some(2));
    assert_eq!(world.revision, 1);
}

#[test]
fn rules_are_ordered_provenanced_cycle_checked_and_budgeted_atomically() {
    let compiled = compile_packages(
        &[base_package(PackageSource::Builtin)],
        PackageLimits::defaults(),
        RuleLimits::defaults(),
    )
    .expect("base rules compile");
    let mut world = world_from_registry(&compiled.registry);
    execute_rules(
        &compiled.registry,
        &mut world,
        "bell_rang",
        RuleLimits::defaults(),
    )
    .expect("bounded rule cascade applies");
    assert_eq!(
        world.effect_log,
        vec!["base.rule.first", "base.rule.second"]
    );
    assert_eq!(counter(&world, "base.parameter.alert"), Some(2));
    assert!(world.provenance.iter().all(|provenance| {
        provenance.principal == "system/rule" && provenance.source_event == "bell_rang"
    }));

    let mut budget_world = world_from_registry(&compiled.registry);
    let before = budget_world.clone();
    assert_eq!(
        execute_rules(
            &compiled.registry,
            &mut budget_world,
            "bell_rang",
            RuleLimits {
                max_applied_effects: 1,
                ..RuleLimits::defaults()
            },
        ),
        Err(ModError::RuleBudget("applied_effects"))
    );
    assert_eq!(budget_world, before);

    let cycle_document = RuleDocument {
        schema_version: CONTENT_SCHEMA_VERSION,
        parameters: Vec::new(),
        events: Vec::new(),
        gameplay_actions: Vec::new(),
        rules: vec![
            RuleDefinition {
                id: "cycle.a".to_owned(),
                priority: 0,
                trigger: TriggerDefinition::WorldEvent {
                    kind: "a".to_owned(),
                },
                predicates: Vec::new(),
                effects: vec![EffectDefinition::EmitEvent {
                    kind: "b".to_owned(),
                }],
            },
            RuleDefinition {
                id: "cycle.b".to_owned(),
                priority: 0,
                trigger: TriggerDefinition::WorldEvent {
                    kind: "b".to_owned(),
                },
                predicates: Vec::new(),
                effects: vec![EffectDefinition::EmitEvent {
                    kind: "a".to_owned(),
                }],
            },
        ],
    };
    let cycle = package(
        PackageSource::Directory,
        manifest("games.example.cycle", "1.0.0"),
        vec![VirtualFile::json("rules/cycle.json", &cycle_document)],
    );
    assert_eq!(
        compile_packages(&[cycle], PackageLimits::defaults(), RuleLimits::defaults(),),
        Err(ModError::Rule("static event cycle".to_owned()))
    );
}

#[test]
fn manifest_rejects_unknown_native_or_service_capabilities() {
    let mut manifest_text = toml::to_string(&manifest("games.example.unsafe", "1.0.0"))
        .expect("fixture manifest serializes");
    manifest_text = manifest_text.replace("\"content\"", "\"shell\"");
    let package = VirtualPackage {
        source: PackageSource::Directory,
        manifest: manifest_text.into_bytes(),
        files: Vec::new(),
    };
    assert!(matches!(
        parse_package(package, PackageLimits::defaults()),
        Err(ModError::Manifest(_))
    ));

    let forbidden_control: Result<RuleDocument, _> = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "parameters": [],
        "events": [],
        "gameplay_actions": [],
        "rules": [],
        "tool_handler": "run_shell"
    }));
    assert!(forbidden_control.is_err());
}
