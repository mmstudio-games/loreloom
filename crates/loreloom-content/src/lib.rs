//! Mod packages, definitions, registries, and pure content compilation.

mod package;
mod registry;
mod schema;
mod world_project;

pub use package::{
    CompiledAgentPrompts, CompiledModSet, CompiledWorldSet, InspectedPackage,
    LORELOOM_ENGINE_VERSION, MOD_MANIFEST_SCHEMA_V1, ModCapability, ModDependency, ModManifest,
    ModManifestDraft, PackageCompiler, PackageError, PackageLimits, PackagePayload,
    PackageResources, PackageSource, PatchDeclaration, PromptManifest, VirtualPackage,
};

pub use registry::{
    CONTENT_SCHEMA_V1, CharacterCompileRequest, ContentError, ContentPackContext,
    DefinitionRegistry, DraftCompileRequest, RegisteredDefinition, SceneCharacterSpawnPlan,
    ScenePlaceSpawnPlan, SceneSpawnPlan, parse_content_hash,
};
pub use schema::*;
pub use world_project::{
    WORLD_MANIFEST_SCHEMA_V1, WorldManifest, WorldProjectError, WorldProjectSource,
};
