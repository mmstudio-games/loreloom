# Changelog

<!-- semifold:release version=0.1.0-alpha.0 -->
## v0.1.0-alpha.0

### Chores

- [`c91fc7e`](https://github.com/mmstudio-games/loreloom/commit/c91fc7eadc85469f0bd3812c4d3c1638828552b1): Validate the fixed public Git revisions for Armillae/Bevy and the SurrealDB/SurrealKV Toasty driver with deterministic P0 integration spikes, then freeze the first-stage durable commit and recovery ordering in the Active Runtime Spec.
- [`2b4a40e`](https://github.com/mmstudio-games/loreloom/commit/2b4a40e196895715103289ee4f6e3c9391c5c568): Initialize the Loreloom Cargo workspace, crate boundaries, version policy, Semifold configuration, and implementation gates.

### New Features

- [`48ae109`](https://github.com/mmstudio-games/loreloom/commit/48ae109376f72ef8c7de4927f95b4ba9e4a187a5): Add strict Agent protocols and budgets, ordered Armillae tool continuation with wakeable cancellation, trusted transcript commands, content-resolved immutable view models, and a durable Narrator-to-NPC Runtime over the SurrealKV world service.
- [`496b192`](https://github.com/mmstudio-games/loreloom/commit/496b192c2c14b3cf8286f136a052a9114fc6fc42): Rebuild saves against candidate world and Mod content before atomically adopting compatible locks without advancing the world revision. Keep the prior manifest unchanged when candidate definitions cannot reconstruct the durable world, while continuing to reject a different root world identity or schema.
- [`803e159`](https://github.com/mmstudio-games/loreloom/commit/803e159d5b39023daccd9198a7cfbc62c7df6f43): Add typed save manifests and mod locks, versioned World commands and events, a Stable ID based Bevy working world with item, skill, clock, spawn, and promotion execution, and a SurrealKV save store with explicit durable transactions, revision conflicts, Action ID idempotency, checksums, checkpoints, record replay, and typed transcript recovery.
- [`ccf768f`](https://github.com/mmstudio-games/loreloom/commit/ccf768f4a7e21bb445a669f9e6d39c660dbf5842): Load the single playable world from root-level manifest, content, and prompt files; persist its exact WorldLock separately from enabled extension Mods; inject world-owned narration and language policy into Narrator and NPC turns; and remove production demo content and mock bridges. Keep all pre-release save and domain payload structures on their initial v1, rejecting obsolete development saves instead of adding compatibility migrations.

### Dependencies

- Update loreloom-core to 0.1.0-alpha.0.
<!-- semifold:release:end -->
