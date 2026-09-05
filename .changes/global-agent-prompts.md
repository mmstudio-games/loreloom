---
loreloom-agent: "patch:feat"
loreloom-content: "patch:feat"
loreloom-runtime: "patch:feat"
loreloom: "patch:feat"
---

Let root worlds and enabled Mods declare optional ordered Narrator and NPC global context through
the same `[prompts]` manifest table. Allow a world to omit or leave both lists empty so an enabled
Mod may supply the first prompt, and inject no engine-authored natural-language system message.
Append Mod prompts deterministically by dependency topology without granting Tools or capabilities,
include their source files in content locks, and keep undeclared prompt resources out of Agent
requests.
