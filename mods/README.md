# Loreloom Mods

This directory is reserved for Mod Packages distributed with Loreloom. Built-in virtual packages
and external directory packages use the same manifest, hash, dependency, patch, registry, and
`ModLock` compiler.

The first package format uses this layout:

```text
mod.toml
content/*.json
rules/*.json
patches/*.json
locales/*.json
assets/**
```

`mod.toml` schema v1 declares the Mod and Pack IDs, SemVer/engine compatibility, content schema,
dependencies, `content`/`rules` capabilities, explicit patches, and the canonical payload SHA-256.
Only configured package roots are loaded; archives, symlinks, path traversal, native libraries,
scripts, network access, shell access, and package-provided Tool handlers are not supported.

Run the local demo with one or more external packages using:

```sh
loreloom --save .loreloom/demo-save --mod /path/to/package --headless "Look around"
```

An existing save reopens only when the complete candidate `ModLock` matches exactly. Package
authoring details and the canonical hash input are defined by
[`runtime.md`](../.agents/specs/runtime.md#104-mod-加载冲突与扩展边界). Never place Provider keys or
other credentials in a package.
