---
loreloom-agent: "patch:feat"
loreloom-content: "patch:feat"
loreloom-core: "patch:feat"
loreloom-runtime: "patch:feat"
loreloom-store: "patch:feat"
loreloom: "patch:feat"
---

Load the single playable world from root-level manifest, content, and prompt files; persist its exact WorldLock separately from enabled extension Mods; inject world-owned narration and language policy into Narrator and NPC turns; and remove production demo content and mock bridges. Keep all pre-release save and domain payload structures on their initial v1, rejecting obsolete development saves instead of adding compatibility migrations.
