---
loreloom: "patch:chore"
loreloom-agent: "patch:chore"
loreloom-core: "patch:chore"
loreloom-runtime: "patch:chore"
loreloom-tui: "patch:chore"
---

Freeze safe real-time Tool Activity as an ephemeral Runtime-to-TUI progress protocol. Tool calls become pending immediately before execution and settle after their result, while raw arguments, ToolResult data, Provider text streams, Transcript state, and persistence remain outside the activity channel.
