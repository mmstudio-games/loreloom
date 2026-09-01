---
loreloom: "patch:chore"
loreloom-runtime: "patch:chore"
---

Document the deferred content reconciliation contract for editable prompts, additive world content, and changing Mod sets. WorldLock and ModLock differences will trigger candidate reconstruction and validation rather than unconditional save rejection; runtime behavior remains unchanged until the tracked implementation task lands.
