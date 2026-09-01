---
loreloom: "patch:fix"
loreloom-agent: "patch:feat"
loreloom-content: "patch:fix"
loreloom-runtime: "patch:fix"
---

Make generated NPC drafts minimal and recoverable: trusted agent profiles now come only from the
locked generation policy, empty role collections may be omitted, knowledge and goal schemas match
their Rust wire types, and invalid drafts report safe field categories. End the generation stage on
the first accepted draft instead of spending another model call on discarded prose.
