use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroU32,
    path::{Path, PathBuf},
    sync::Arc,
};

use loreloom_agent::AgentDefinition;
use loreloom_content::{
    AgentProfileDefinition, AttributeDefinition, CONTENT_SCHEMA_V1, ContainerDefinition,
    ContentDocument, Definition, DefinitionRegistry, ItemDefinition, LORELOOM_ENGINE_VERSION,
    MOD_MANIFEST_SCHEMA_V1, ModCapability, ModManifestDraft, PackageCompiler, PackagePayload,
    PackageSource, PlaceDefinition, ResourceDefinition, ResourceMaximumPolicy, SceneDefinition,
    VirtualPackage,
};
use loreloom_core::{
    ActionState, ActorId, AgentBinding, AttributeAdjustment, AttributeOperation, AutonomyMode,
    BaseAttributes, CharacterController, CharacterLifetime, CharacterProfile, CharacterRecord,
    ContentDefinitionId, ContentOrigin, DisplayName, DomainRecord, EntityOrigin, Fixed, LifeState,
    LongText, ModId, ObjectId, PlaceRecord, Posture, ResourcePool, SAVE_FORMAT_V1, SaveId,
    SaveManifest, SceneRecord, SessionId, ShortText, StackState, SystemIdGenerator, UiSnapshot,
    WorldId, WorldStateRecord, WorldTime,
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
    let candidate_mod_lock = content.manifest.mod_lock.clone();
    let store = if path.exists() {
        SaveStore::open(path).await?
    } else {
        SaveStore::create(path, content.manifest, content.records).await?
    };
    let service = WorldService::open(
        store,
        content.registry,
        &candidate_mod_lock,
        content.world_config,
    )
    .await?;
    let mut id_generator = SystemIdGenerator;
    let session_id = SessionId::generate_with(&mut id_generator)?;
    let narrator = Arc::new(DemoNarratorBridge::new(content.npc_id, content.scene_id));
    let npc = Arc::new(DemoNpcBridge);
    let mut runtime = GameRuntime::new(service, narrator, session_id, RuntimeConfig::default());
    runtime.register_npc(
        content.npc_id,
        AgentDefinition {
            profile_id: content.agent_profile_id,
            system_style: LongText::new("Speak as the innkeeper Mira, with grounded brevity.")?,
            model_alias: short("loreloom-demo-npc")?,
            allowed_tools: BTreeSet::from(["advance_time".to_owned()]),
        },
        npc,
    );
    let initial_snapshot = runtime.initial_snapshot().await?;
    Ok(DemoSetup {
        runtime,
        initial_snapshot,
    })
}

struct DemoContent {
    registry: DefinitionRegistry,
    records: Vec<DomainRecord>,
    manifest: SaveManifest,
    world_config: WorldConfig,
    npc_id: ActorId,
    scene_id: ObjectId,
    agent_profile_id: ContentDefinitionId,
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
            Definition::Scene(SceneDefinition {
                id: scene_definition.clone(),
                display_name: display("The Rainbound Inn")?,
                framing: short("An inn holds its breath under steady rain.")?,
                entry_place: place_definition.clone(),
                places: BTreeSet::from([place_definition.clone()]),
                characters: Vec::new(),
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

    let player_id = ActorId::from(object_id("2b3c")?);
    let npc_id = ActorId::from(object_id("2b3d")?);
    let scene_id = object_id("2b3e")?;
    let place_id = object_id("2b3f")?;
    let player_root = object_id("2b40")?;
    let npc_root = object_id("2b41")?;
    let world_id = parse_id::<WorldId>("wld_01890f6a-2b42-7d4e-8f90-123456789abc")?;
    let origin = |id: &ContentDefinitionId| -> Result<ContentOrigin, AppError> {
        registry
            .get(id)
            .map(|definition| definition.origin.clone())
            .ok_or(AppError::Arguments("demo definition is missing"))
    };
    let mut player_resources = BTreeMap::new();
    player_resources.insert(
        resource_id.clone(),
        ResourcePool {
            resource_id: resource_id.clone(),
            current: Fixed::from_integer(8)?,
            base_maximum: Fixed::from_integer(12)?,
        },
    );
    let records = vec![
        DomainRecord::WorldState(WorldStateRecord {
            id: world_id,
            player_actor: player_id,
            active_scene: scene_id,
            clock: WorldTime::ZERO,
            rng_seed: [11; 32],
        }),
        DomainRecord::Scene(SceneRecord {
            id: scene_id,
            display_name: display("The Rainbound Inn")?,
            framing: short("An inn holds its breath under steady rain.")?,
            entry_place: place_id,
            active: true,
            origin: origin(&scene_definition)?,
        }),
        DomainRecord::Place(PlaceRecord {
            id: place_id,
            scene_id,
            display_name: display("Hearth Room")?,
            description: short("Rain whispers beyond a low, warm hearth.")?,
            tags: BTreeSet::new(),
            origin: origin(&place_definition)?,
        }),
        DomainRecord::Character(CharacterRecord {
            id: player_id,
            display_name: display("Traveler")?,
            profile: profile("A traveler newly arrived from the rain.")?,
            controller: CharacterController::Player,
            lifetime: CharacterLifetime::Persistent,
            location: place_id,
            inventory_root: player_root,
            agent_binding: None,
            base_attributes: BaseAttributes(BTreeMap::from([(
                attribute_id.clone(),
                Fixed::from_integer(10)?,
            )])),
            attribute_adjustments: vec![AttributeAdjustment {
                source_id: object_id("2b43")?,
                attribute_id: attribute_id.clone(),
                operation: AttributeOperation::Flat,
                value: Fixed::ONE,
                priority: 0,
            }],
            resources: player_resources,
            life_state: LifeState::Alive,
            action_state: ActionState::Idle,
            posture: Posture::Standing,
            origin: system_origin()?,
        }),
        DomainRecord::Character(CharacterRecord {
            id: npc_id,
            display_name: display("Mira")?,
            profile: profile("The observant keeper of the rainbound inn.")?,
            controller: CharacterController::Agent,
            lifetime: CharacterLifetime::Persistent,
            location: place_id,
            inventory_root: npc_root,
            agent_binding: Some(AgentBinding {
                profile_id: agent_profile_id.clone(),
                enabled: true,
                autonomy: AutonomyMode::Directed,
            }),
            base_attributes: BaseAttributes::default(),
            attribute_adjustments: Vec::new(),
            resources: BTreeMap::new(),
            life_state: LifeState::Alive,
            action_state: ActionState::Idle,
            posture: Posture::Standing,
            origin: system_origin()?,
        }),
        inventory(
            player_root,
            player_id,
            place_id,
            &inventory_definition,
            origin(&inventory_definition)?,
        )?,
        inventory(
            npc_root,
            npc_id,
            place_id,
            &inventory_definition,
            origin(&inventory_definition)?,
        )?,
    ];
    Ok(DemoContent {
        registry,
        records,
        manifest: SaveManifest {
            format_version: SAVE_FORMAT_V1,
            save_id: parse_id::<SaveId>("sav_01890f6a-2b44-7d4e-8f90-123456789abc")?,
            world_id,
            mod_lock,
        },
        world_config: WorldConfig {
            inventory_root_definition: inventory_definition,
            spawn_system_definition: definition_id("system", "spawn")?,
        },
        npc_id,
        scene_id,
        agent_profile_id,
    })
}

fn inventory(
    id: ObjectId,
    owner: ActorId,
    place: ObjectId,
    definition_id: &ContentDefinitionId,
    origin: ContentOrigin,
) -> Result<DomainRecord, AppError> {
    Ok(DomainRecord::Item(loreloom_core::ItemRecord {
        id,
        definition_id: definition_id.clone(),
        stack: StackState(NonZeroU32::MIN),
        durability: None,
        container: Some(loreloom_core::ContainerState {
            max_weight_grams: Fixed::from_integer(10_000)?,
            max_children: 32,
        }),
        contained_by: None,
        owned_by: Some(owner),
        equipped: None,
        located_at: Some(place),
        custom_name: None,
        bound_actor: Some(owner),
        parameters: BTreeMap::new(),
        instance_adjustments: Vec::new(),
        origin: EntityOrigin::Content { origin },
    }))
}

fn profile(summary: &str) -> Result<CharacterProfile, AppError> {
    Ok(CharacterProfile {
        summary: short(summary)?,
        values: Vec::new(),
        speaking_style: short("Plain and direct.")?,
        narrative_tags: BTreeSet::new(),
    })
}

fn system_origin() -> Result<EntityOrigin, AppError> {
    Ok(EntityOrigin::System {
        source: definition_id("system", "bootstrap")?,
    })
}

fn definition_id(kind: &str, key: &str) -> Result<ContentDefinitionId, AppError> {
    Ok(ContentDefinitionId::parse(format!(
        "games.loreloom.demo:{kind}/{key}"
    ))?)
}

fn object_id(suffix: &str) -> Result<ObjectId, AppError> {
    parse_id(&format!("obj_01890f6a-{suffix}-7d4e-8f90-123456789abc"))
}

fn parse_id<T>(value: &str) -> Result<T, AppError>
where
    T: std::str::FromStr<Err = loreloom_core::IdentityError>,
{
    Ok(value.parse()?)
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
        assert_eq!(content.manifest.mod_lock.mods.len(), 2);
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
