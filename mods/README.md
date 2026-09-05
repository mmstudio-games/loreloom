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
appearance/pack.toml
appearance/images/*.png
assets/**
```

`mod.toml` schema v1 declares the Mod and Pack IDs, SemVer/engine compatibility, content schema,
dependencies, `content`/`rules` capabilities, explicit patches, and the canonical payload SHA-256.
To append global context to the Narrator or every NPC Agent, declare prompt files explicitly:

```toml
[prompts]
narrator = ["prompts/narrator.md"]
npc = ["prompts/npc.md"]
```

Both lists are optional. Their order is preserved, and enabled Mods are appended after the root
world in dependency-topology order. Prompt files that exist in the package but are not declared in
these lists remain ordinary resources and are not injected into an Agent request. Prompt text does
not register Tools or grant capabilities.

Only explicitly enabled package roots are loaded; directory presence alone does not enable a Mod.
Archives, symlinks, path traversal, native libraries,
scripts, network access, shell access, and package-provided Tool handlers are not supported.

## Dynamic appearance

An appearance package keeps using the ordinary `mod.toml`; there is no separate `package.toml`.
Declare the `appearance` capability and list both the pack and its PNG files in the package payload.
Character definitions may contain an optional `appearance` field in content schema v1. The runtime
stores only the semantic model ID and parameter values; generated RGBA pixels and terminal protocol
data are never persisted.

The fixed v1 pack entry point is `appearance/pack.toml`. A minimal pack looks like:

```toml
schema_version = 1
pack_id = "example.looks:appearance_pack/main"

[[models]]
id = "example.looks:appearance_model/player"
canvas_width = 128
canvas_height = 192

[[models.parameters]]
id = "example.looks:appearance_parameter/eyes"
value = { type = "color", rgb = [70, 110, 140] }

[[models.frames]]
name = "awake"
duration_ms = 950

[[models.frames.layers]]
name = "body"
z_index = 0
source = "appearance/images/body.png"

[[models.frames.layers]]
name = "eyes"
z_index = 10
source = "appearance/images/eyes.png"
mask = "appearance/images/eyes-mask.png"
tint = { type = "parameter", id = "example.looks:appearance_parameter/eyes" }
blend = "source_over"
```

Models may define up to 16 synchronized frames and use `source_over`, `multiply`, `screen`, or
`hard_light` blending. Layers support alpha masks, fixed or parameter-driven tint, opacity, and a
typed equality predicate through `when`. PNG sprite strips place equal-width canvas frames next to
each other; `source_frame` and `mask_frame` select the frame. All paths must remain under
`appearance/images/` and use `.png`.

The TUI composes and encodes portraits on a background worker. `[tui].image_protocol` accepts
`auto`, `kitty`, `iterm2`, `sixel`, `halfblocks`, or `disabled`; `auto` is recommended for direct
terminal runs. Wrappers and nested terminals can hide protocol capabilities, in which case select a
protocol explicitly or run Loreloom directly in the terminal. Narrow layouts remain text-only.

Third-party art is not part of Loreloom merely because the pack format can load it. Package authors
must have redistribution rights and should record the asset license and attribution alongside their
package.

Run the root world with one or more external packages using:

```sh
loreloom --config loreloom.toml --save .loreloom/world --mod /path/to/package --headless "Look around"
```

An existing save reopens only when the candidate `WorldLock` and complete extension `ModLock` match
exactly. Package
authoring details and the canonical hash input are defined by
[`runtime.md`](../.agents/specs/runtime.md#104-mod-加载冲突与扩展边界). Never place Provider keys or
other credentials in a package.
