//! Mod packages, definitions, registries, and pure content compilation.

mod package;
mod registry;
mod schema;

pub use package::{
    CompiledModSet, LORELOOM_ENGINE_VERSION, MOD_MANIFEST_SCHEMA_V1, ModCapability, ModDependency,
    ModManifest, ModManifestDraft, PackageCompiler, PackageError, PackageLimits, PackagePayload,
    PackageResources, PackageSource, PatchDeclaration, VirtualPackage,
};

pub use registry::{
    CONTENT_SCHEMA_V1, CharacterCompileRequest, ContentError, ContentPackContext,
    DefinitionRegistry, DraftCompileRequest, RegisteredDefinition, parse_content_hash,
};
pub use schema::*;
