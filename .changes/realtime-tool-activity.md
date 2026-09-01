---
loreloom: "patch:feat"
loreloom-agent: "patch:feat"
loreloom-core: "patch:feat"
loreloom-runtime: "patch:feat"
loreloom-tui: "patch:feat"
---

Publish safe Tool Activity as each accepted ToolCall starts and settles instead of waiting for the player turn to finish. Propagate the current activity list through Runtime and the non-blocking UI adapter, render live pending and terminal states without raw arguments or results, and keep the final display ordered between player input and Narrator prose.
