---
loreloom: "patch:fix"
loreloom-tui: "patch:fix"
---

Make launcher Settings editable for providers, credential references, resource budgets, and TUI preferences. Validate and atomically save changes to the selected configuration file and apply them to the current launch.

Preserve drafts after validation or write failures, discard canceled edits, and reject saves when the configuration has changed externally. Saving normalizes TOML formatting and removes comments.
