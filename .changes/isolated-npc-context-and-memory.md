---
loreloom-core: "patch:fix"
loreloom-world: "patch:fix"
loreloom-agent: "patch:fix"
loreloom-runtime: "patch:fix"
loreloom: "patch:fix"
---

Isolate NPC context and persistent dialogue by actor ownership. Deliver player input only to requested NPCs present when it occurred, persist each NPC response privately, filter before budgeting, and validate ownership again at the model boundary.

Remove free-text request_npc_turn assignments and exclude unscoped global events from NPC context. Transcript v1 now requires an explicit audience; older development saves without it are rejected rather than granting implicit access. Verify isolation, reload, newcomer exclusion, forged ownership, and failed memory commit recovery.
