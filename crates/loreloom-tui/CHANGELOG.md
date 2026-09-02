# Changelog

<!-- semifold:release version=0.1.0-alpha.0 -->
## v0.1.0-alpha.0

### Bug Fixes

- [`1391848`](https://github.com/mmstudio-games/loreloom/commit/1391848513ac06c72c773fe9806e9f69cd4c7290): Remove the internal world revision number from the player-facing TUI header and footer while retaining revision tracking for Runtime consistency and diagnostics.
- [`cf9fb65`](https://github.com/mmstudio-games/loreloom/commit/cf9fb65c238a430545ae157fdfa74d0d61435f2f): Make the Mods overlay reachable on default macOS terminals by adding Ctrl+O as the displayed portable shortcut and recognizing the character emitted by Option+M, while retaining F2 and Alt+M.
- [`5083fb5`](https://github.com/mmstudio-games/loreloom/commit/5083fb56a4f07d1cea4a50759f0aae14f34dd4a8): Define a terminal-portable Mods overlay shortcut contract with Ctrl+O and F2 as reliable entries, while retaining Alt+M and macOS Option+M compatibility where the terminal can distinguish them.
- [`10efea3`](https://github.com/mmstudio-games/loreloom/commit/10efea35adc9103b98152e7c7968ce356cdd2d4b): Render an accepted player input immediately before the thinking row and reconcile it with the authoritative Runtime snapshot.
- [`d0a314f`](https://github.com/mmstudio-games/loreloom/commit/d0a314fdc0a52f4ea00c758ec1d0a7f030f808d8): Anchor the transcript at the latest story content by default and make PageUp, PageDown, and mouse wheel scrolling operate within the actual wrapped viewport. Preserve local reading distance across Runtime snapshots, clamp it after resize, and expose concise older/latest navigation hints.

### Chores

- [`c3c2401`](https://github.com/mmstudio-games/loreloom/commit/c3c240182372934c11d77db687e47073a813e2d2): Freeze a read-only Mods overlay and safe startup package catalog. Keep the root World separate, distinguish enabled ModLock entries from valid installed-but-disabled directory packages, summarize unavailable candidates without exposing paths or content, and reserve Alt+M or F2 for viewing and scrolling without changing Runtime or persistent state.
- [`cace98a`](https://github.com/mmstudio-games/loreloom/commit/cace98a3df1c020c64477d38cbc3df9d120eb39c): Validate the responsive Ratatui/Crossterm terminal interaction boundary with deterministic wide and narrow snapshots, grapheme-safe multiline editing, streaming and tool states, resize preservation, and symmetric terminal recovery.
- [`b930503`](https://github.com/mmstudio-games/loreloom/commit/b93050356875b9ec413f48bda885ff276c218ab9): Freeze safe real-time Tool Activity as an ephemeral Runtime-to-TUI progress protocol. Tool calls become pending immediately before execution and settle after their result, while raw arguments, ToolResult data, Provider text streams, Transcript state, and persistence remain outside the activity channel.
- [`2b4a40e`](https://github.com/mmstudio-games/loreloom/commit/2b4a40e196895715103289ee4f6e3c9391c5c568): Initialize the Loreloom Cargo workspace, crate boundaries, version policy, Semifold configuration, and implementation gates.

### New Features

- [`6fab674`](https://github.com/mmstudio-games/loreloom/commit/6fab67407304ef678269363bf30f4df1f7c41517): Hide Condition names from character and UI projections until the observing Actor owns a confirmed diagnosis KnownFact, while retaining intensity-filtered symptoms and loading the engine diagnosis Tag through the normal built-in Mod pipeline.
- [`93d6db8`](https://github.com/mmstudio-games/loreloom/commit/93d6db8fde448bcf66eb450f6596b35e10ce8d0d): Add a read-only, scrollable Mods overlay with Alt+M and F2 shortcuts. Show the root World, enabled extensions, valid installed-but-disabled packages, and an unavailable-candidate summary while keeping editor input, Runtime cancellation, Transcript scrolling, and persistent state isolated.
- [`08a96d0`](https://github.com/mmstudio-games/loreloom/commit/08a96d0a72a9a7208fa0f61ec9d12cef7f48f20a): Project validated per-Mod content summaries from compiled enabled packages and inspected installed packages into the Mods overlay, including definition totals, non-zero categories, prompts, and patches without persisting package metadata or exposing package contents.
- [`344daca`](https://github.com/mmstudio-games/loreloom/commit/344daca0179106d7e50944008a458000d2e55c78): Define non-persistent per-Mod content summaries for the Mods overlay, counting owned top-level definitions by player-relevant category plus declared narrator/NPC prompts and patches.
- [`95634c7`](https://github.com/mmstudio-games/loreloom/commit/95634c7d3f71c9192f2353fe93fc5a6fff161961): Redesign the terminal UI around narrative reading with a compact state sidebar, label-free narrator prose, a right-pane composer, local thinking animation, subdued tool activity, human-readable world status, and responsive narrow State/Story pages. Add deterministic wide and narrow visual snapshots plus multiline composer coverage.
- [`7197245`](https://github.com/mmstudio-games/loreloom/commit/719724597653f1805c6de82e935e9a3dfd068acc): Publish safe Tool Activity as each accepted ToolCall starts and settles instead of waiting for the player turn to finish. Propagate the current activity list through Runtime and the non-blocking UI adapter, render live pending and terminal states without raw arguments or results, and keep the final display ordered between player input and Narrator prose.
- [`e29eb70`](https://github.com/mmstudio-games/loreloom/commit/e29eb705717bfa2218f470975b7c83645e0e9b70): Publish coarse runtime phase events throughout player-turn orchestration and render them as local animated thinking status. Remove Provider text streaming from the product UI protocol, preserve committed snapshots as the only source of story and world state, and keep submit, cancel, resize, and terminal-state handling responsive while model calls are in flight.
- [`c71e7d4`](https://github.com/mmstudio-games/loreloom/commit/c71e7d451c4c996637b2291a6a9b40beea7ad94a): Simplify narrator NPC orchestration to `create_npc` and `request_npc_turn`, while keeping materialization, generation, replanning, and NPC execution inside the runtime. Move the default generation policy into locked world content, derive scene and agent bindings from authoritative state, advertise only currently schedulable actor IDs, and surface sanitized tool rejection codes in the TUI.
- [`744aa84`](https://github.com/mmstudio-games/loreloom/commit/744aa8464effdf48c892aaf3c7d5545ee9abcf5a): Define the default world launcher, compatible save catalog, and deterministic fixed, preset, or UGC player-creation flow with declarative typed fields and bounded initialization effects.
- [`336418d`](https://github.com/mmstudio-games/loreloom/commit/336418de267bf45c3d4ebcca77418b93560be492): Add the production terminal UI, non-blocking Runtime client adapter, stable cross-turn cancellation, and a persistent headless demo executable.
- [`9822a60`](https://github.com/mmstudio-games/loreloom/commit/9822a60ca671d308f6731d7999749023c5a5bbb0): Open interactive play in a world launcher with compatible save discovery, then create fixed, preset, or typed UGC player characters through a deterministic terminal flow before Provider or Store startup.

### Dependencies

- Update loreloom-core to 0.1.0-alpha.0.
<!-- semifold:release:end -->
