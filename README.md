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

Loreloom has entered workspace bootstrap. Its accepted architecture and active runtime baseline are
recorded in:

- [Design index](.agents/DESIGN.md)
- [RFC 0001: persistent agentic world and TUI architecture](.agents/rfcs/0001-loreloom-architecture.md)
- [Active runtime specification](.agents/specs/runtime.md)
- [Runtime implementation checklist](.agents/todos/runtime.md)

RFC 0001 is Accepted and the runtime specification is Active. Unresolved protocol details remain
scoped implementation gates and do not receive default answers from workspace scaffolding.

## Rust policy

Loreloom follows the latest stable Rust toolchain and Rust 2024 edition. The project does not define
or test an MSRV and must not add a `rust-version` compatibility promise.
