---
loreloom: "patch:fix"
loreloom-agent: "patch:feat"
loreloom-content: "patch:feat"
loreloom-core: "patch:feat"
loreloom-runtime: "patch:feat"
loreloom-tui: "patch:feat"
---

Simplify narrator NPC orchestration to `create_npc` and `request_npc_turn`, while keeping materialization, generation, replanning, and NPC execution inside the runtime. Move the default generation policy into locked world content, derive scene and agent bindings from authoritative state, advertise only currently schedulable actor IDs, and surface sanitized tool rejection codes in the TUI.
