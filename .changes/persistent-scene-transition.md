---
loreloom-core: "patch:feat"
loreloom-runtime: "patch:feat"
loreloom-world: "patch:feat"
---

Add atomic narrator-driven scene transitions for existing and content-defined scenes. Preserve inactive scenes and their owned entities across revisits, materialize each scene definition once, move the existing player without duplicating bootstrap data, emit explicit scene lifecycle events, and force narrator replanning after every transition attempt.
