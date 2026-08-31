# Loreloom

> A persistent agentic world woven from state and story.

Loreloom is a Rust terminal game in which a Bevy ECS world is the source of gameplay truth and
LLM-powered characters act through explicit Tools. Character state, relationships, knowledge,
inventory, locations, goals, and other facts that affect future play belong to structured world
state instead of being left for a model to remember from prompt history.

The game builds on [Armillae](https://github.com/mmstudio-games/armillae), an open-source Rust
infrastructure project that provides Bevy ECS simulation, provider-independent LLM calls, and Tool
execution. Loreloom is an independent project and owns its game domain, Agent harness, persistence
policy, player experience, and TUI. A clean Loreloom checkout must build using publicly resolvable
dependencies.

## Status

Loreloom now has a playable vertical slice: durable SurrealKV saves, content and rule Mods, a
Bevy ECS working world, Narrator/NPC orchestration, declarative gameplay Tools, and a responsive
two-pane TUI. Its accepted architecture and active runtime baseline are recorded in:

- [Design index](.agents/DESIGN.md)
- [RFC 0001: persistent agentic world and TUI architecture](.agents/rfcs/0001-loreloom-architecture.md)
- [Active runtime specification](.agents/specs/runtime.md)
- [Runtime implementation checklist](.agents/todos/runtime.md)

RFC 0001 is Accepted and the runtime specification is Active. Deterministic Store shutdown and the
physical backup/restore/save-switch APIs remain gated on the configured database driver.

## Run

The no-Secret demo uses deterministic local model bridges:

```sh
cargo run -p loreloom -- --save .loreloom/demo-save
```

Use `--headless "your input"` for a single non-TTY turn, and repeat `--mod PATH` to add explicit
directory Mod package roots. Existing saves must reopen with the same exact ModLock.

For real models, copy [loreloom.example.toml](loreloom.example.toml), keep credentials in the named
environment variable or a referenced Secret file, and run:

```sh
cargo run -p loreloom -- --config loreloom.toml --save .loreloom/world
```

The strict config rejects unknown fields and raw `api_key`/`token` values. Custom endpoints also
require an exact `allowed_endpoint_hosts` entry; non-loopback custom endpoints require HTTPS.

## Rust policy

Loreloom follows the latest stable Rust toolchain and Rust 2024 edition. The project does not define
or test an MSRV and must not add a `rust-version` compatibility promise.
