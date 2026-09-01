---
loreloom: "patch:feat"
loreloom-agent: "patch:feat"
loreloom-runtime: "patch:feat"
---

Start NPC materialization immediately after an accepted `create_npc` call. Successful orchestration
tools now end their model turn through a ToolResult barrier, skipping unused prose and later calls
from the same response before the runtime replans against the committed character.
