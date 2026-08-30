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

Loreloom is in design discovery. The initial architecture is recorded in:

- [Design index](.agents/DESIGN.md)
- [RFC 0001: persistent agentic world and TUI architecture](.agents/rfcs/0001-loreloom-architecture.md)
- [Proposed runtime specification](.agents/specs/runtime.md)

The RFC is Draft and the specification is Proposed. They must be reviewed and accepted before
product code or implementation TODOs are created.

## Rust policy

Loreloom follows the latest stable Rust toolchain and Rust 2024 edition. The project does not define
or test an MSRV and must not add a `rust-version` compatibility promise.
