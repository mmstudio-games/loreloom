---
loreloom-content: "patch:fix"
---

Treat a directory Mod's root `.gitignore` as local authoring metadata instead of an unsupported
package payload, without relaxing validation for other unknown or nested files.
