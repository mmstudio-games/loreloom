---
loreloom-agent: "patch:feat"
loreloom-content: "patch:feat"
loreloom-runtime: "patch:feat"
loreloom: "patch:feat"
---

Let root worlds and enabled Mods declare ordered Narrator and NPC global context through the same
`[prompts]` manifest table. Append Mod prompts deterministically by dependency topology without
granting Tools or capabilities, include their source files in content locks, and keep undeclared
prompt resources out of Agent requests.
