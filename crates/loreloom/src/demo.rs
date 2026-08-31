use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroU32,
    path::{Path, PathBuf},
    sync::Arc,
};

use loreloom_agent::AgentDefinition;
use loreloom_content::{
    AgentProfileDefinition, AttributeDefinition, CONTENT_SCHEMA_V1, CharacterDefinition,
    ContainerDefinition, ContentDocument, Definition, DefinitionRegistry,
    InitialCharacterController, InitialCharacterLifetime, InitialResource, ItemDefinition,
    LORELOOM_ENGINE_VERSION, MOD_MANIFEST_SCHEMA_V1, ModCapability, ModManifestDraft,
    PackageCompiler, PackagePayload, PackageSource, PlaceDefinition, ResourceDefinition,
    ResourceMaximumPolicy, SceneCharacterDefinition, SceneDefinition, VirtualPackage,
};
use loreloom_core::{
    ActorId, AttributeOperation, AutonomyMode, BaseAttributes, CharacterController,
    CharacterProfile, ContentDefinitionId, DisplayName, DomainRecord, EntityOrigin, Fixed,
    LongText, ModId, ModLock, ObjectId, SaveId, SessionId, ShortText, SpawnConstraints,
    SystemIdGenerator, UiSnapshot,
};
use loreloom_runtime::{GameRuntime, RuntimeConfig, WorldService};
use loreloom_store::SaveStore;
use loreloom_world::WorldConfig;
use semver::{Version, VersionReq};

use crate::{
    bridge::{DemoNarratorBridge, DemoNpcBridge},
    error::AppError,
};

pub struct DemoSetup {
    pub runtime: GameRuntime,
    pub initial_snapshot: UiSnapshot,
}

pub async fn build_demo(path: &Path, mod_paths: &[PathBuf]) -> Result<DemoSetup, AppError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    let content = demo_content(mod_paths)?;
    let candidate_mod_lock = content.mod_lock.clone();
    let mut ids = SystemIdGenerator;
    let (service, initial_records) = if path.exists() {
        let mut store = SaveStore::open(path).await?;
        if store.manifest().mod_lock != candidate_mod_lock {
            return Err(AppError::Runtime(
                loreloom_runtime::RuntimeError::ContentLockMismatch,
            ));
        }
        let loaded = store.load().await?;
        let service = WorldService::open(
            store,
            content.registry,
            &candidate_mod_lock,
            content.world_config,
        )
        .await?;
        (service, loaded.records)
    } else {
        let plan = content
            .registry
            .compile_scene(&content.scene_definition_id)?;
        let (service, bootstrap) = WorldService::create(
            path,
            SaveId::generate_with(&mut ids)?,
            candidate_mod_lock.clone(),
            content.registry,
            &plan,
            [11; 32],
            content.world_config,
        )
        .await?;
        (service, bootstrap.records)
    };
    let (scene_id, npc_id) = demo_runtime_ids(&initial_records, &content.npc_definition_id)?;
    let session_id = SessionId::generate_with(&mut ids)?;
    let narrator = Arc::new(DemoNarratorBridge::new(npc_id, scene_id));
    let npc = Arc::new(DemoNpcBridge);
    let mut runtime = GameRuntime::new(service, narrator, session_id, RuntimeConfig::default());
    runtime.register_npc(npc_id, content.agent_definition, npc);
    let initial_snapshot = runtime.initial_snapshot().await?;
    Ok(DemoSetup {
        runtime,
        initial_snapshot,
    })
}

fn demo_runtime_ids(
    records: &[DomainRecord],
    npc_definition_id: &ContentDefinitionId,
) -> Result<(ObjectId, ActorId), AppError> {
    let scene_id = records
        .iter()
        .find_map(|record| match record {
            DomainRecord::WorldState(state) => Some(state.active_scene),
            _ => None,
        })
        .ok_or(AppError::Arguments("demo WorldState is missing"))?;
    let npc_id = records
        .iter()
        .find_map(|record| match record {
            DomainRecord::Character(character)
                if character.controller == CharacterController::Agent
                    && matches!(
                        &character.origin,
                        EntityOrigin::Content { origin }
                            if &origin.definition_id == npc_definition_id
                    ) =>
            {
                Some(character.id)
            }
            _ => None,
        })
        .ok_or(AppError::Arguments("demo NPC is missing"))?;
    Ok((scene_id, npc_id))
}

struct DemoContent {
    registry: DefinitionRegistry,
    mod_lock: ModLock,
    world_config: WorldConfig,
    scene_definition_id: ContentDefinitionId,
    npc_definition_id: ContentDefinitionId,
    agent_definition: AgentDefinition,
}

fn demo_content(mod_paths: &[PathBuf]) -> Result<DemoContent, AppError> {
    let mod_id = ModId::parse("games.loreloom.demo")?;
    let pack_id = definition_id("pack", "demo")?;
    let agent_profile_id = definition_id("agent_profile", "mira")?;
    let attribute_id = definition_id("attribute", "resolve")?;
    let resource_id = definition_id("resource", "stamina")?;
    let inventory_definition = definition_id("item", "inventory")?;
    let place_definition = definition_id("place", "hearth")?;
    let scene_definition = definition_id("scene", "rainy_inn")?;
    let player_definition = definition_id("character", "traveler")?;
    let npc_definition = definition_id("character", "mira")?;
    let document = ContentDocument {
        schema_version: CONTENT_SCHEMA_V1,
        definitions: vec![
            Definition::AgentProfile(AgentProfileDefinition {
                id: agent_profile_id.clone(),
                display_name: display("Mira")?,
                system_style: short("Measured, observant, and economical with words.")?,
                model_alias: short("loreloom-demo-npc")?,
                tool_capabilities: BTreeSet::from([short("advance_time")?]),
                autonomy: AutonomyMode::Directed,
            }),
            Definition::Attribute(AttributeDefinition {
                id: attribute_id.clone(),
                display_name: display("Resolve")?,
                minimum: Fixed::ZERO,
                maximum: Fixed::from_integer(20)?,
                allowed_operations: BTreeSet::from([AttributeOperation::Flat]),
            }),
            Definition::Resource(ResourceDefinition {
                id: resource_id.clone(),
                display_name: display("Stamina")?,
                minimum: Fixed::ZERO,
                maximum: Fixed::from_integer(100)?,
                maximum_policy: ResourceMaximumPolicy::ClampCurrent,
                derived_from_attribute: None,
            }),
            Definition::Item(ItemDefinition {
                id: inventory_definition.clone(),
                display_name: display("Inventory")?,
                description: short("A private inventory root.")?,
                tags: BTreeSet::new(),
                stack_limit: NonZeroU32::MIN,
                unit_weight_grams: Fixed::ZERO,
                durability: None,
                container: Some(ContainerDefinition {
                    max_weight_grams: Fixed::from_integer(10_000)?,
                    max_children: 32,
                }),
                equipment_slots: BTreeSet::new(),
                modifiers: Vec::new(),
            }),
            Definition::Place(PlaceDefinition {
                id: place_definition.clone(),
                display_name: display("Hearth Room")?,
                description: short("Rain whispers beyond a low, warm hearth.")?,
                tags: BTreeSet::new(),
                edges: BTreeSet::new(),
            }),
            Definition::Character(CharacterDefinition {
                id: player_definition.clone(),
                display_name: display("Traveler")?,
                profile: profile("A traveler newly arrived from the rain.")?,
                agent_profile: None,
                base_attributes: BaseAttributes(BTreeMap::from([(
                    attribute_id.clone(),
                    Fixed::from_integer(10)?,
                )])),
                resources: vec![InitialResource {
                    resource_id: resource_id.clone(),
                    current: Fixed::from_integer(8)?,
                    base_maximum: Fixed::from_integer(12)?,
                }],
                conditions: Vec::new(),
                inventory: Vec::new(),
                skills: Vec::new(),
                knowledge: Vec::new(),
                goals: Vec::new(),
                spawn_constraints: SpawnConstraints {
                    minimum_attributes: BTreeMap::from([(attribute_id.clone(), Fixed::ZERO)]),
                    maximum_attributes: BTreeMap::from([(
                        attribute_id.clone(),
                        Fixed::from_integer(20)?,
                    )]),
                    maximum_attribute_points: Fixed::from_integer(20)?,
                    maximum_items: 8,
                    maximum_skills: 4,
                    allowed_definitions: BTreeSet::new(),
                },
            }),
            Definition::Character(CharacterDefinition {
                id: npc_definition.clone(),
                display_name: display("Mira")?,
                profile: profile("The observant keeper of the rainbound inn.")?,
                agent_profile: Some(agent_profile_id.clone()),
                base_attributes: BaseAttributes::default(),
                resources: Vec::new(),
                conditions: Vec::new(),
                inventory: Vec::new(),
                skills: Vec::new(),
                knowledge: Vec::new(),
                goals: Vec::new(),
                spawn_constraints: SpawnConstraints {
                    minimum_attributes: BTreeMap::new(),
                    maximum_attributes: BTreeMap::new(),
                    maximum_attribute_points: Fixed::ZERO,
                    maximum_items: 8,
                    maximum_skills: 4,
                    allowed_definitions: BTreeSet::new(),
                },
            }),
            Definition::Scene(SceneDefinition {
                id: scene_definition.clone(),
                display_name: display("The Rainbound Inn")?,
                framing: short("An inn holds its breath under steady rain.")?,
                entry_place: place_definition.clone(),
                places: BTreeSet::from([place_definition.clone()]),
                characters: vec![
                    SceneCharacterDefinition {
                        local_key: short("player")?,
                        character_id: player_definition,
                        place_id: place_definition.clone(),
                        controller: InitialCharacterController::Player,
                        lifetime: InitialCharacterLifetime::Persistent,
                    },
                    SceneCharacterDefinition {
                        local_key: short("mira")?,
                        character_id: npc_definition.clone(),
                        place_id: place_definition.clone(),
                        controller: InitialCharacterController::Agent,
                        lifetime: InitialCharacterLifetime::Persistent,
                    },
                ],
            }),
        ],
    };
    let package = VirtualPackage::builtin(
        ModManifestDraft {
            schema_version: MOD_MANIFEST_SCHEMA_V1,
            mod_id,
            version: Version::new(1, 0, 0),
            pack_id,
            engine: VersionReq::parse(&format!("={LORELOOM_ENGINE_VERSION}"))
                .map_err(|_| AppError::Arguments("demo engine requirement is invalid"))?,
            content_schema: CONTENT_SCHEMA_V1,
            dependencies: Vec::new(),
            capabilities: vec![ModCapability::Content],
            patches: Vec::new(),
        },
        vec![PackagePayload::new(
            "content/demo.json",
            serde_json::to_vec(&document).map_err(AppError::DemoCodec)?,
        )],
    )?;
    let mut sources = vec![PackageSource::Builtin(package)];
    sources.extend(mod_paths.iter().cloned().map(PackageSource::Directory));
    let (registry, mod_lock, _) = PackageCompiler::default().compile(sources)?.into_parts();
    let agent_profile = registry
        .get(&agent_profile_id)
        .and_then(|entry| match &entry.definition {
            Definition::AgentProfile(profile) => Some(profile),
            _ => None,
        })
        .ok_or(AppError::Arguments("demo AgentProfile is missing"))?;
    let agent_definition = AgentDefinition {
        profile_id: agent_profile.id.clone(),
        system_style: LongText::new(agent_profile.system_style.as_str())?,
        model_alias: agent_profile.model_alias.clone(),
        allowed_tools: agent_profile
            .tool_capabilities
            .iter()
            .map(|capability| capability.as_str().to_owned())
            .collect(),
    };
    Ok(DemoContent {
        registry,
        mod_lock,
        world_config: WorldConfig {
            inventory_root_definition: inventory_definition,
            spawn_system_definition: definition_id("system", "spawn")?,
            rule_limits: Default::default(),
        },
        scene_definition_id: scene_definition,
        npc_definition_id: npc_definition,
        agent_definition,
    })
}

fn profile(summary: &str) -> Result<CharacterProfile, AppError> {
    Ok(CharacterProfile {
        summary: short(summary)?,
        values: Vec::new(),
        speaking_style: short("Plain and direct.")?,
        narrative_tags: BTreeSet::new(),
    })
}

fn definition_id(kind: &str, key: &str) -> Result<ContentDefinitionId, AppError> {
    Ok(ContentDefinitionId::parse(format!(
        "games.loreloom.demo:{kind}/{key}"
    ))?)
}

fn display(value: &str) -> Result<DisplayName, AppError> {
    Ok(DisplayName::new(value)?)
}

fn short(value: &str) -> Result<ShortText, AppError> {
    Ok(ShortText::new(value)?)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use loreloom_content::{ModDependency, TagDefinition};

    use super::*;

    #[test]
    fn external_directory_mod_joins_demo_registry_lock_and_durable_turn() {
        let temporary = tempfile::tempdir().expect("temporary demo root");
        let package_root = temporary.path().join("weather-mod");
        let package = weather_package();
        write_package(&package_root, &package);

        let content =
            demo_content(std::slice::from_ref(&package_root)).expect("compiled demo Mods");
        assert_eq!(content.mod_lock.mods.len(), 2);
        assert!(
            content
                .registry
                .get(
                    &"games.loreloom.weather:tag/external-rain"
                        .parse()
                        .expect("external tag ID"),
                )
                .is_some()
        );

        let io = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        let mut setup = io
            .block_on(build_demo(
                &temporary.path().join("save"),
                std::slice::from_ref(&package_root),
            ))
            .expect("demo with external Mod");
        let outcome = io
            .block_on(
                setup
                    .runtime
                    .handle_player_input("Ask Mira about the weather."),
            )
            .expect("durable demo turn");
        assert_eq!(outcome.snapshot.revision, loreloom_core::Revision::new(3));
    }

    #[test]
    fn failed_content_bootstrap_does_not_create_a_partial_save() {
        let temporary = tempfile::tempdir().expect("temporary demo root");
        let save_path = temporary.path().join("save");
        let content = demo_content(&[]).expect("compiled demo content");
        let plan = content
            .registry
            .compile_scene(&content.scene_definition_id)
            .expect("compiled scene plan");
        let invalid_config = WorldConfig {
            inventory_root_definition: definition_id("item", "missing")
                .expect("missing definition ID"),
            spawn_system_definition: content.world_config.spawn_system_definition,
            rule_limits: Default::default(),
        };
        let io = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        let result = io.block_on(WorldService::create(
            &save_path,
            SaveId::new(),
            content.mod_lock,
            content.registry,
            &plan,
            [0; 32],
            invalid_config,
        ));

        assert!(matches!(
            result,
            Err(loreloom_runtime::RuntimeError::World(
                loreloom_world::WorldError::DefinitionNotFound { .. }
            ))
        ));
        assert!(!save_path.exists());
    }

    fn weather_package() -> VirtualPackage {
        let owner = ModId::parse("games.loreloom.weather").expect("weather Mod ID");
        let document = ContentDocument {
            schema_version: CONTENT_SCHEMA_V1,
            definitions: vec![Definition::Tag(TagDefinition {
                id: ContentDefinitionId::new(&owner, "tag", "external-rain")
                    .expect("weather Tag ID"),
                display_name: DisplayName::new("External Rain").expect("weather display name"),
            })],
        };
        VirtualPackage::builtin(
            ModManifestDraft {
                schema_version: MOD_MANIFEST_SCHEMA_V1,
                mod_id: owner.clone(),
                version: Version::new(1, 0, 0),
                pack_id: ContentDefinitionId::new(&owner, "pack", "main").expect("weather Pack ID"),
                engine: VersionReq::parse(&format!("={LORELOOM_ENGINE_VERSION}"))
                    .expect("engine requirement"),
                content_schema: CONTENT_SCHEMA_V1,
                dependencies: vec![ModDependency {
                    mod_id: ModId::parse("games.loreloom.demo").expect("demo Mod ID"),
                    requirement: VersionReq::parse("=1.0.0").expect("demo requirement"),
                    optional: false,
                }],
                capabilities: vec![ModCapability::Content],
                patches: Vec::new(),
            },
            vec![PackagePayload::new(
                "content/weather.json",
                serde_json::to_vec(&document).expect("weather Content Document"),
            )],
        )
        .expect("sealed weather package")
    }

    fn write_package(root: &Path, package: &VirtualPackage) {
        fs::create_dir_all(root).expect("package root");
        fs::write(root.join("mod.toml"), package.manifest_bytes()).expect("package Manifest");
        for payload in package.payloads() {
            let path = root.join(&payload.path);
            fs::create_dir_all(path.parent().expect("payload parent")).expect("payload directory");
            fs::write(path, &payload.bytes).expect("package payload");
        }
    }
}
