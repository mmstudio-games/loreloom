---
loreloom: "patch:chore"
loreloom-content: "patch:chore"
loreloom-core: "patch:chore"
loreloom-runtime: "patch:chore"
loreloom-tui: "patch:chore"
---

Freeze a read-only Mods overlay and safe startup package catalog. Keep the root World separate,
distinguish enabled ModLock entries from valid installed-but-disabled directory packages, summarize
unavailable candidates without exposing paths or content, and reserve Alt+M or F2 for viewing and
scrolling without changing Runtime or persistent state.
