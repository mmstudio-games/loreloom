# Loreloom Mods

This directory is reserved for extensions to the single root world. The root `world.toml`,
`content/`, `rules/`, and `prompts/` are the game itself and are locked by `WorldLock`; packages
below `mods/` are optional additions locked separately by `ModLock`.

The first package format uses this layout:

```text
mod.toml
content/*.json
rules/*.json
patches/*.json
locales/*.json
prompts/*.md
assets/**
```

`mod.toml` schema v1 declares the Mod and Pack IDs, SemVer/engine compatibility, content schema,
dependencies, `content`/`rules` capabilities, explicit patches, and the canonical payload SHA-256.
Only explicitly enabled package roots are loaded; directory presence alone does not enable a Mod.
Archives, symlinks, path traversal, native libraries,
scripts, network access, shell access, and package-provided Tool handlers are not supported.

Run the root world with one or more external packages using:

```sh
loreloom --config loreloom.toml --save .loreloom/world --mod /path/to/package --headless "Look around"
```

An existing save reopens only when the candidate `WorldLock` and complete extension `ModLock` match
exactly. Package
authoring details and the canonical hash input are defined by
[`runtime.md`](../.agents/specs/runtime.md#104-mod-加载冲突与扩展边界). Never place Provider keys or
other credentials in a package.
