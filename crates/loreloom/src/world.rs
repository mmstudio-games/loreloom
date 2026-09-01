use std::{collections::BTreeSet, path::Path, path::PathBuf};

use loreloom_agent::NarratorDefinition;
use loreloom_content::{
    CONTENT_SCHEMA_V1, ContentDocument, Definition, LORELOOM_ENGINE_VERSION,
    MOD_MANIFEST_SCHEMA_V1, ModCapability, ModManifestDraft, PackageCompiler, PackagePayload,
    PackageSource, TagDefinition, VirtualPackage, WorldProjectSource,
};
use loreloom_core::{
    ContentDefinitionId, DIAGNOSED_CONDITION_PREDICATE_ID, DisplayName, ModId, SaveId, SessionId,
    SystemIdGenerator, UiSnapshot,
};
use loreloom_runtime::{GameRuntime, WorldService};
use loreloom_store::SaveStore;
use loreloom_world::WorldConfig;
use semver::{Version, VersionReq};

use crate::{config::ConfiguredProviders, error::AppError};

pub struct WorldSetup {
    pub runtime: GameRuntime,
    pub initial_snapshot: UiSnapshot,
    #[cfg(test)]
    world_lock: loreloom_core::WorldLock,
    #[cfg(test)]
    mod_lock: loreloom_core::ModLock,
}

pub async fn build_world_with(
    world_root: &Path,
    save_path: &Path,
    mod_paths: &[PathBuf],
    mut configured: ConfiguredProviders,
) -> Result<WorldSetup, AppError> {
    let world_source = WorldProjectSource::load(world_root)?;
    let core_id = ModId::parse("games.loreloom.core")?;
    let engine_namespaces = BTreeSet::from([core_id]);
    let compiled = PackageCompiler::default().compile_world(
        &world_source,
        [PackageSource::Builtin(core_package()?)],
        mod_paths.iter().cloned().map(PackageSource::Directory),
        &engine_namespaces,
    )?;
    let prompts = compiled.prompts().clone();
    let (registry, world_lock, mod_lock, _) = compiled.into_parts();
    #[cfg(test)]
    let test_world_lock = world_lock.clone();
    #[cfg(test)]
    let test_mod_lock = mod_lock.clone();
    let manifest = world_source.manifest();
    let generation_policy = registry
        .get(&manifest.npc_generation_policy)
        .and_then(|entry| match &entry.definition {
            Definition::GenerationPolicy(policy) => Some(policy.clone()),
            _ => None,
        })
        .ok_or(AppError::WorldPolicy(
            "default NPC generation policy is unavailable",
        ))?;
    configured.runtime.generation_policy = Some(generation_policy);
    let plan = registry.compile_scene(&manifest.initial_scene)?;
    let world_config = WorldConfig {
        inventory_root_definition: manifest.inventory_root_definition.clone(),
        spawn_system_definition: manifest.spawn_system_definition.clone(),
        rule_limits: configured.rules,
    };
    let (narrator_prompts, npc_prompts) = prompts.into_parts();
    let narrator_definition = NarratorDefinition {
        narrator_prompts,
        npc_prompts,
    };

    if let Some(parent) = save_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    let mut ids = SystemIdGenerator;
    let service = if save_path.exists() {
        WorldService::open(
            SaveStore::open(save_path).await?,
            registry,
            &world_lock,
            &mod_lock,
            world_config,
        )
        .await?
    } else {
        WorldService::create(
            save_path,
            SaveId::generate_with(&mut ids)?,
            world_lock,
            mod_lock,
            registry,
            &plan,
            [11; 32],
            world_config,
        )
        .await?
        .0
    };
    let session_id = SessionId::generate_with(&mut ids)?;
    let mut runtime = GameRuntime::new(
        service,
        configured.narrator,
        narrator_definition,
        session_id,
        configured.runtime,
    );
    runtime.set_default_npc_bridge(configured.npc);
    let initial_snapshot = runtime.initial_snapshot().await?;
    Ok(WorldSetup {
        runtime,
        initial_snapshot,
        #[cfg(test)]
        world_lock: test_world_lock,
        #[cfg(test)]
        mod_lock: test_mod_lock,
    })
}

fn core_package() -> Result<VirtualPackage, AppError> {
    let mod_id = ModId::parse("games.loreloom.core")?;
    let document = ContentDocument {
        schema_version: CONTENT_SCHEMA_V1,
        definitions: vec![Definition::Tag(TagDefinition {
            id: ContentDefinitionId::parse(DIAGNOSED_CONDITION_PREDICATE_ID)?,
            display_name: DisplayName::new("Diagnosed condition")?,
        })],
    };
    Ok(VirtualPackage::builtin(
        ModManifestDraft {
            schema_version: MOD_MANIFEST_SCHEMA_V1,
            mod_id: mod_id.clone(),
            version: Version::new(1, 0, 0),
            pack_id: ContentDefinitionId::new(&mod_id, "pack", "core")?,
            engine: VersionReq::parse(&format!("={LORELOOM_ENGINE_VERSION}"))
                .map_err(|_| AppError::Arguments("engine requirement is invalid"))?,
            content_schema: CONTENT_SCHEMA_V1,
            dependencies: Vec::new(),
            capabilities: vec![ModCapability::Content],
            patches: Vec::new(),
            prompts: loreloom_content::PromptManifest::default(),
        },
        vec![PackagePayload::new(
            "content/core.json",
            serde_json::to_vec(&document).map_err(AppError::ContentCodec)?,
        )],
    )?)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use armillae_core::{CompletionRequest, CompletionResponse};
    use armillae_llm::{
        BoxFuture, BridgeCapabilities, BridgeError, CompletionStream, LlmBridge, ProjectionReport,
    };

    use super::*;

    struct NeverCalledBridge;

    impl LlmBridge for NeverCalledBridge {
        fn capabilities(&self) -> BridgeCapabilities {
            BridgeCapabilities::all()
        }

        fn project(&self, _request: &CompletionRequest) -> Result<ProjectionReport, BridgeError> {
            Ok(ProjectionReport::exact("world-assembly-test"))
        }

        fn complete<'a>(
            &'a self,
            _request: CompletionRequest,
        ) -> BoxFuture<'a, Result<CompletionResponse, BridgeError>> {
            Box::pin(async {
                Err(BridgeError::InvalidRequest {
                    message: "model call is not expected".to_owned(),
                })
            })
        }

        fn stream<'a>(
            &'a self,
            _request: CompletionRequest,
        ) -> BoxFuture<'a, Result<CompletionStream, BridgeError>> {
            Box::pin(async {
                Err(BridgeError::InvalidRequest {
                    message: "streaming is not expected".to_owned(),
                })
            })
        }
    }

    fn configured() -> ConfiguredProviders {
        let bridge: Arc<dyn LlmBridge> = Arc::new(NeverCalledBridge);
        ConfiguredProviders {
            narrator: Arc::clone(&bridge),
            npc: bridge,
            runtime: Default::default(),
            rules: Default::default(),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn root_world_creates_with_separate_locks() {
        let temporary = tempfile::tempdir().expect("save parent");
        let save_path = temporary.path().join("save");
        let world_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let setup = build_world_with(&world_root, &save_path, &[], configured())
            .await
            .expect("create root world");
        assert_eq!(setup.initial_snapshot.player.display_name.as_str(), "旅人");
        assert_eq!(
            setup.world_lock.world_id.as_str(),
            "games.loreloom.rainbound-inn"
        );
        assert!(setup.mod_lock.mods.is_empty());
    }
}
