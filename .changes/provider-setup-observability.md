---
loreloom: "patch:fix"
---

Report actionable Provider setup failures before opening a World or Save. Distinguish Narrator and
NPC configuration, missing or empty environment credentials, unreadable credential files, endpoint
policy failures, unsupported Providers, and adapter rejection while keeping Secret values and
upstream free-text errors out of diagnostics.
