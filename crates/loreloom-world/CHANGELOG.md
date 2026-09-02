# Changelog

<!-- semifold:release version=0.1.0-alpha.0 -->
## v0.1.0-alpha.0

### Chores

- [`83fc5a3`](https://github.com/mmstudio-games/loreloom/commit/83fc5a39b256acbb17800a13c653c3ee20ed6ad1): Freeze Scene as a persistent narrative container and Place as a connected location node. Document Narrator-owned runtime Scene/Place creation, bidirectional Place edges, generated provenance, delayed execution, and mandatory replanning before implementation.
- [`c91fc7e`](https://github.com/mmstudio-games/loreloom/commit/c91fc7eadc85469f0bd3812c4d3c1638828552b1): Validate the fixed public Git revisions for Armillae/Bevy and the SurrealDB/SurrealKV Toasty driver with deterministic P0 integration spikes, then freeze the first-stage durable commit and recovery ordering in the Active Runtime Spec.
- [`2b4a40e`](https://github.com/mmstudio-games/loreloom/commit/2b4a40e196895715103289ee4f6e3c9391c5c568): Initialize the Loreloom Cargo workspace, crate boundaries, version policy, Semifold configuration, and implementation gates.

### New Features

- [`dad8c8a`](https://github.com/mmstudio-games/loreloom/commit/dad8c8a854eaba4b7b98e40431529c540a75d49a): Execute Active Skill resource costs, complete declarative effect plans, cooldowns, events, and rule cascades as one rollback-safe world candidate.
- [`48ae109`](https://github.com/mmstudio-games/loreloom/commit/48ae109376f72ef8c7de4927f95b4ba9e4a187a5): Add strict Agent protocols and budgets, ordered Armillae tool continuation with wakeable cancellation, trusted transcript commands, content-resolved immutable view models, and a durable Narrator-to-NPC Runtime over the SurrealKV world service.
- [`9f09b2e`](https://github.com/mmstudio-games/loreloom/commit/9f09b2e4f0658dc1414c174950d0d0dce362a6b5): Settle condition periodic effects and expiry at deterministic world-clock boundaries, including same-tick periodic-first ordering, target effects, structured events, and candidate rollback.
- [`6fab674`](https://github.com/mmstudio-games/loreloom/commit/6fab67407304ef678269363bf30f4df1f7c41517): Hide Condition names from character and UI projections until the observing Actor owns a confirmed diagnosis KnownFact, while retaining intensity-filtered symptoms and loading the engine diagnosis Tag through the normal built-in Mod pipeline.
- [`62ba87c`](https://github.com/mmstudio-games/loreloom/commit/62ba87cb62fc453c18c39a426e6bfabffe17883f): Execute typed event options, capability-gated gameplay actions, save/session parameters, and deterministic declarative rule cascades through durable runtime tools.
- [`5754b85`](https://github.com/mmstudio-games/loreloom/commit/5754b850a1307a080e869ecf90a773d5bc6b8297): Persist bidirectional Place connections, expose adjacent destinations in Scene context, and require ordinary movement to follow those edges. Add Narrator-only delayed tools that atomically create generated inactive Scenes or connected Places, preserve generated provenance, and replan from the committed topology before transition or movement.
- [`803e159`](https://github.com/mmstudio-games/loreloom/commit/803e159d5b39023daccd9198a7cfbc62c7df6f43): Add typed save manifests and mod locks, versioned World commands and events, a Stable ID based Bevy working world with item, skill, clock, spawn, and promotion execution, and a SurrealKV save store with explicit durable transactions, revision conflicts, Action ID idempotency, checksums, checkpoints, record replay, and typed transcript recovery.
- [`0c7755b`](https://github.com/mmstudio-games/loreloom/commit/0c7755b8260ec0d465b35d3a3f27b199d801e296): Add atomic narrator-driven scene transitions for existing and content-defined scenes. Preserve inactive scenes and their owned entities across revisits, materialize each scene definition once, move the existing player without duplicating bootstrap data, emit explicit scene lifecycle events, and force narrator replanning after every transition attempt.
- [`f844dd4`](https://github.com/mmstudio-games/loreloom/commit/f844dd442d36222b09bd5475bb8fb8c02a183809): Add fixed, preset, and typed UGC player bootstraps. Content authors can declare deterministic player forms whose validated fields compile into the existing character and parameter records, including durable player-created provenance.
- [`b0dfd1a`](https://github.com/mmstudio-games/loreloom/commit/b0dfd1a59409079f2dd696859d1a8550dc7ef583): Compile initial scenes into deterministic spawn plans and create validated Revision 0 worlds through the shared character factory.
- [`744aa84`](https://github.com/mmstudio-games/loreloom/commit/744aa8464effdf48c892aaf3c7d5545ee9abcf5a): Define the default world launcher, compatible save catalog, and deterministic fixed, preset, or UGC player-creation flow with declarative typed fields and bounded initialization effects.

### Dependencies

- Update loreloom-content to 0.1.0-alpha.0.
- Update loreloom-core to 0.1.0-alpha.0.
<!-- semifold:release:end -->
