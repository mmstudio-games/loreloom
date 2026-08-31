---
loreloom: "patch:fix"
loreloom-agent: "patch:fix"
loreloom-core: "patch:fix"
loreloom-runtime: "patch:fix"
---

Preserve sanitized model-call failure categories, invocation stages, safe Provider metadata, and a
shared `err_` correlation ID from Armillae through AgentRunner and Runtime. Show the same diagnostic
in TUI/headless failures, return NPC failures to the Narrator without exposing raw Provider text,
and ignore local Provider configurations and save data.
