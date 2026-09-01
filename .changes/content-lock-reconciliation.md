---
loreloom-store: "patch:feat"
loreloom-runtime: "patch:feat"
---

Rebuild saves against candidate world and Mod content before atomically adopting compatible locks without advancing the world revision. Keep the prior manifest unchanged when candidate definitions cannot reconstruct the durable world, while continuing to reject a different root world identity or schema.
