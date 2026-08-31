---
loreloom-runtime: "patch:fix"
---

Let the narrator discover exact scene transition targets before requesting a switch. Reject invented or stale targets with actionable recovery metadata, treat repeated accepted targets idempotently, and prevent narration from claiming arrival before the transition commits.
