use std::{fs, path::Path};

use loreloom_content::{
    CONTENT_SCHEMA_V1, ContentDocument, ContentError, Definition, MOD_MANIFEST_SCHEMA_V1,
    ModCapability, ModDependency, ModManifestDraft, PackageCompiler, PackageError, PackageLimits,
    PackagePayload, PackageSource, PatchDeclaration, PromptManifest, TagDefinition, VirtualPackage,
};
use loreloom_core::{ContentDefinitionId, DisplayName, ModId, ModSourceKind};
use semver::{Version, VersionReq};
use tempfile::TempDir;

fn mod_id(value: &str) -> ModId {
    ModId::parse(value).expect("fixture Mod ID")
}

fn definition_id(value: &str) -> ContentDefinitionId {
    value.parse().expect("fixture Definition ID")
}

fn requirement(value: &str) -> VersionReq {
    VersionReq::parse(value).expect("fixture version requirement")
}

fn tag(id: &str, display_name: &str) -> Definition {
    Definition::Tag(TagDefinition {
        id: definition_id(id),
        display_name: DisplayName::new(display_name).expect("fixture display name"),
    })
}

fn draft(id: &str, capabilities: Vec<ModCapability>) -> ModManifestDraft {
    let owner = mod_id(id);
    ModManifestDraft {
        schema_version: MOD_MANIFEST_SCHEMA_V1,
        pack_id: ContentDefinitionId::new(&owner, "pack", "main").expect("fixture Pack ID"),
        mod_id: owner,
        version: Version::new(1, 0, 0),
        engine: requirement("=0.1.0"),
        content_schema: CONTENT_SCHEMA_V1,
        dependencies: Vec::new(),
        capabilities,
        patches: Vec::new(),
        prompts: PromptManifest::default(),
    }
}

fn document_payload(path: &str, definitions: Vec<Definition>) -> PackagePayload {
    PackagePayload::new(
        path,
        serde_json::to_vec(&ContentDocument {
            schema_version: CONTENT_SCHEMA_V1,
            definitions,
        })
        .expect("fixture Content Document"),
    )
}

fn base_package() -> VirtualPackage {
    let mut manifest = draft("games.loreloom.base", vec![ModCapability::Content]);
    manifest.prompts.narrator = vec!["prompts/narrator.md".to_owned()];
    VirtualPackage::builtin(
        manifest,
        vec![
            document_payload(
                "content/base.json",
                vec![tag("games.loreloom.base:tag/rain", "Rain")],
            ),
            PackagePayload::new("assets/weather/rain.txt", b"steady rain".to_vec()),
            PackagePayload::new("locales/en.json", br#"{"rain":"Rain"}"#.to_vec()),
            PackagePayload::new("prompts/narrator.md", "Narrate steady rain.".as_bytes()),
        ],
    )
    .expect("sealed base package")
}

fn patch_package(replacement: Definition) -> VirtualPackage {
    let mut manifest = draft("games.loreloom.patch", vec![ModCapability::Content]);
    manifest.dependencies = vec![ModDependency {
        mod_id: mod_id("games.loreloom.base"),
        requirement: requirement("=1.0.0"),
        optional: false,
    }];
    manifest.patches = vec![PatchDeclaration {
        id: definition_id("games.loreloom.patch:patch/rain-name"),
        file: "patches/rain-name.json".to_owned(),
        target_mod: mod_id("games.loreloom.base"),
        target_version: requirement("=1.0.0"),
        target_definition: definition_id("games.loreloom.base:tag/rain"),
    }];
    VirtualPackage::builtin(
        manifest,
        vec![PackagePayload::new(
            "patches/rain-name.json",
            serde_json::to_vec(&serde_json::json!({
                "schema_version": CONTENT_SCHEMA_V1,
                "operations": [{
                    "operation": "replace_definition",
                    "value": replacement,
                }],
            }))
            .expect("fixture Patch Document"),
        )],
    )
    .expect("sealed patch package")
}

fn write_directory(root: &Path, package: &VirtualPackage) {
    fs::create_dir_all(root).expect("package root");
    fs::write(root.join("mod.toml"), package.manifest_bytes()).expect("manifest");
    for payload in package.payloads() {
        let path = root.join(&payload.path);
        fs::create_dir_all(path.parent().expect("payload parent")).expect("payload directory");
        fs::write(path, &payload.bytes).expect("payload");
    }
}

#[test]
fn builtin_and_directory_packages_share_dependency_patch_registry_and_lock_pipeline() {
    let temporary = TempDir::new().expect("temporary package root");
    let external = patch_package(tag("games.loreloom.base:tag/rain", "Silver Rain"));
    write_directory(temporary.path(), &external);

    let compiler = PackageCompiler::default();
    let compiled = compiler
        .compile([
            PackageSource::Directory(temporary.path().to_owned()),
            PackageSource::Builtin(base_package()),
        ])
        .expect("compile package closure independent of source order");
    let rain = compiled
        .registry()
        .get(&definition_id("games.loreloom.base:tag/rain"))
        .expect("patched definition");
    let Definition::Tag(rain) = &rain.definition else {
        panic!("fixture remains a Tag Definition");
    };
    assert_eq!(rain.display_name.as_str(), "Silver Rain");
    assert_eq!(
        compiled
            .registry()
            .get(&rain.id)
            .expect("patched origin")
            .origin
            .mod_id,
        mod_id("games.loreloom.base")
    );
    assert_eq!(compiled.registry().contexts().count(), 2);
    assert_eq!(compiled.mod_lock().mods.len(), 2);
    assert_eq!(
        compiled.mod_lock().mods[0].source_kind,
        ModSourceKind::Builtin
    );
    assert_eq!(
        compiled.mod_lock().mods[1].source_kind,
        ModSourceKind::Directory
    );
    assert_eq!(compiled.mod_lock().mods[1].dependencies.len(), 1);
    assert_eq!(
        compiled.mod_lock().mods[1].applied_patches,
        vec![definition_id("games.loreloom.patch:patch/rain-name")]
    );
    assert_eq!(
        compiled
            .resources()
            .get(&mod_id("games.loreloom.base"), "assets/weather/rain.txt"),
        Some(b"steady rain".as_slice())
    );
    assert_eq!(
        compiled
            .resources()
            .get(&mod_id("games.loreloom.base"), "prompts/narrator.md"),
        Some("Narrate steady rain.".as_bytes())
    );
    assert_eq!(
        compiled
            .prompts()
            .narrator()
            .iter()
            .map(|prompt| prompt.as_str())
            .collect::<Vec<_>>(),
        ["Narrate steady rain."]
    );
    assert!(compiled.prompts().npc().is_empty());

    compiler
        .compile_locked(
            [
                PackageSource::Builtin(base_package()),
                PackageSource::Directory(temporary.path().to_owned()),
            ],
            compiled.mod_lock(),
        )
        .expect("exact lock reopens");
    let mut wrong_lock = compiled.mod_lock().clone();
    wrong_lock.mods.pop();
    assert!(matches!(
        compiler.compile_locked(
            [
                PackageSource::Builtin(base_package()),
                PackageSource::Directory(temporary.path().to_owned()),
            ],
            &wrong_lock,
        ),
        Err(PackageError::LockMismatch)
    ));
}

#[test]
fn canonical_hash_ignores_manifest_whitespace_but_covers_payload_bytes_and_paths() {
    let base = base_package();
    let mut spaced_manifest = base.manifest_bytes().to_vec();
    spaced_manifest.extend_from_slice(b"\n\n");
    PackageCompiler::default()
        .compile([PackageSource::Builtin(VirtualPackage::from_raw(
            spaced_manifest,
            base.payloads().to_vec(),
        ))])
        .expect("TOML whitespace is outside the canonical hash");

    let mut tampered = base.payloads().to_vec();
    tampered[0].bytes.push(b' ');
    assert!(matches!(
        PackageCompiler::default().compile([PackageSource::Builtin(VirtualPackage::from_raw(
            base.manifest_bytes().to_vec(),
            tampered,
        ))]),
        Err(PackageError::HashMismatch { .. })
    ));

    assert!(matches!(
        PackageCompiler::default().compile([PackageSource::Builtin(VirtualPackage::from_raw(
            base.manifest_bytes().to_vec(),
            vec![PackagePayload::new("../escape.json", b"{}".to_vec())],
        ))]),
        Err(PackageError::UnsafePath)
    ));
}

#[test]
fn dependencies_and_full_definition_patch_contract_fail_before_registry_publication() {
    let patch = patch_package(tag("games.loreloom.base:tag/rain", "Silver Rain"));
    assert!(matches!(
        PackageCompiler::default().compile([PackageSource::Builtin(patch)]),
        Err(PackageError::MissingDependency)
    ));

    let wrong_id = patch_package(tag("games.loreloom.base:tag/fog", "Fog"));
    assert!(matches!(
        PackageCompiler::default().compile([
            PackageSource::Builtin(base_package()),
            PackageSource::Builtin(wrong_id),
        ]),
        Err(PackageError::InvalidPatch)
    ));
}

#[test]
fn dependency_cycles_incompatible_optional_dependencies_and_duplicates_are_rejected() {
    let mut left = draft("games.loreloom.left", vec![ModCapability::Content]);
    left.dependencies = vec![ModDependency {
        mod_id: mod_id("games.loreloom.right"),
        requirement: requirement("=1.0.0"),
        optional: false,
    }];
    let mut right = draft("games.loreloom.right", vec![ModCapability::Content]);
    right.dependencies = vec![ModDependency {
        mod_id: mod_id("games.loreloom.left"),
        requirement: requirement("=1.0.0"),
        optional: false,
    }];
    let left = VirtualPackage::builtin(
        left,
        vec![document_payload(
            "content/left.json",
            vec![tag("games.loreloom.left:tag/value", "Left")],
        )],
    )
    .expect("left package");
    let right = VirtualPackage::builtin(
        right,
        vec![document_payload(
            "content/right.json",
            vec![tag("games.loreloom.right:tag/value", "Right")],
        )],
    )
    .expect("right package");
    assert!(matches!(
        PackageCompiler::default()
            .compile([PackageSource::Builtin(left), PackageSource::Builtin(right),]),
        Err(PackageError::DependencyCycle)
    ));

    let mut optional = draft("games.loreloom.optional", vec![ModCapability::Content]);
    optional.dependencies = vec![ModDependency {
        mod_id: mod_id("games.loreloom.base"),
        requirement: requirement("=2.0.0"),
        optional: true,
    }];
    let optional = VirtualPackage::builtin(
        optional,
        vec![document_payload(
            "content/optional.json",
            vec![tag("games.loreloom.optional:tag/value", "Optional")],
        )],
    )
    .expect("optional package");
    assert!(matches!(
        PackageCompiler::default().compile([
            PackageSource::Builtin(base_package()),
            PackageSource::Builtin(optional),
        ]),
        Err(PackageError::IncompatibleDependency)
    ));

    let duplicate_id = "games.loreloom.duplicate:tag/value";
    let duplicate = VirtualPackage::builtin(
        draft("games.loreloom.duplicate", vec![ModCapability::Content]),
        vec![document_payload(
            "content/duplicate.json",
            vec![tag(duplicate_id, "First"), tag(duplicate_id, "Second")],
        )],
    )
    .expect("duplicate package");
    assert!(matches!(
        PackageCompiler::default().compile([PackageSource::Builtin(duplicate)]),
        Err(PackageError::Content(
            ContentError::DuplicateDefinition { .. }
        ))
    ));
}

#[test]
fn definition_groups_capabilities_and_tightened_resource_limits_are_enforced() {
    let wrong_group = VirtualPackage::builtin(
        draft("games.loreloom.wrong-group", vec![ModCapability::Rules]),
        vec![document_payload(
            "rules/static.json",
            vec![tag("games.loreloom.wrong-group:tag/value", "Wrong")],
        )],
    )
    .expect("wrong-group package");
    assert!(matches!(
        PackageCompiler::default().compile([PackageSource::Builtin(wrong_group)]),
        Err(PackageError::InvalidDefinitionGroup)
    ));

    let missing_capability = VirtualPackage::builtin(
        draft(
            "games.loreloom.missing-capability",
            vec![ModCapability::Rules],
        ),
        vec![document_payload(
            "content/static.json",
            vec![tag("games.loreloom.missing-capability:tag/value", "Wrong")],
        )],
    )
    .expect("missing-capability package");
    assert!(matches!(
        PackageCompiler::default().compile([PackageSource::Builtin(missing_capability)]),
        Err(PackageError::UnsafePath)
    ));

    let limited = PackageCompiler::new(
        Version::new(0, 1, 0),
        PackageLimits {
            max_files: 4,
            max_single_file_bytes: 8,
            max_total_bytes: 16,
            max_path_depth: 4,
            max_manifest_bytes: 4_096,
        },
    )
    .expect("tightened limits");
    assert!(matches!(
        limited.compile([PackageSource::Builtin(base_package())]),
        Err(PackageError::ResourceLimit {
            limit: "single_file_bytes"
        })
    ));
}

#[test]
fn prompt_declarations_require_unique_present_non_empty_prompt_files() {
    let invalid = |prompts: PromptManifest, payloads: Vec<PackagePayload>| {
        let mut manifest = draft(
            "games.loreloom.invalid-prompt",
            vec![ModCapability::Content],
        );
        manifest.prompts = prompts;
        PackageCompiler::default().compile([PackageSource::Builtin(
            VirtualPackage::builtin(manifest, payloads).expect("seal invalid fixture"),
        )])
    };

    assert!(matches!(
        invalid(
            PromptManifest {
                narrator: vec!["prompts/shared.md".to_owned()],
                npc: vec!["prompts/shared.md".to_owned()],
            },
            vec![PackagePayload::new("prompts/shared.md", "Shared.")],
        ),
        Err(PackageError::InvalidManifest { field: "prompts" })
    ));
    assert!(matches!(
        invalid(
            PromptManifest {
                narrator: vec!["prompts/missing.md".to_owned()],
                npc: Vec::new(),
            },
            Vec::new(),
        ),
        Err(PackageError::InvalidManifest { field: "prompts" })
    ));
    assert!(matches!(
        invalid(
            PromptManifest {
                narrator: vec!["prompts/empty.md".to_owned()],
                npc: Vec::new(),
            },
            vec![PackagePayload::new("prompts/empty.md", "")],
        ),
        Err(PackageError::InvalidData)
    ));

    let prompt_hash = |text: &str| {
        let mut manifest = draft("games.loreloom.prompt-hash", vec![ModCapability::Content]);
        manifest.prompts.narrator = vec!["prompts/narrator.md".to_owned()];
        let compiled = PackageCompiler::default()
            .compile([PackageSource::Builtin(
                VirtualPackage::builtin(
                    manifest,
                    vec![PackagePayload::new("prompts/narrator.md", text.as_bytes())],
                )
                .expect("seal prompt hash fixture"),
            )])
            .expect("compile prompt hash fixture");
        compiled.mod_lock().mods[0].content_hash.clone()
    };
    assert_ne!(prompt_hash("First prompt."), prompt_hash("Second prompt."));
}

#[cfg(unix)]
#[test]
fn directory_loader_rejects_symlinks_before_hash_or_json_processing() {
    use std::os::unix::fs::symlink;

    let temporary = TempDir::new().expect("temporary package root");
    let package_root = temporary.path().join("package");
    write_directory(&package_root, &base_package());
    let outside = temporary.path().join("outside.txt");
    fs::write(&outside, b"outside").expect("outside file");
    fs::create_dir_all(package_root.join("assets/links")).expect("asset directory");
    symlink(&outside, package_root.join("assets/links/outside.txt")).expect("fixture symlink");

    assert!(matches!(
        PackageCompiler::default().compile([PackageSource::Directory(package_root)]),
        Err(PackageError::Symlink)
    ));
}
