---
name: owner-ui-decisions
description: "UI decisions: no fit-to-width for wide HTML mail (the horizontal scrollbar stays); the accent override is ignored in High contrast"
metadata:
  type: project
---

- **Wide HTML mail:** no fit-to-width scaling. A newsletter wider than the reader keeps its horizontal scrollbar;
  decided as acceptable. Do not propose zoom-to-fit again unless the maintainer asks.
- **Accent in High contrast:** the user's accent override is ignored while the active theme's kind is high contrast
  (fixed, Follow Windows, user themes extending it); `theme.kdl` keeps it, Settings → Appearance disables the accent
  with a note (ADR 0161, `accent_ignored` / `accentIgnored`).

**Why:** decisions on the P1 / P2 human-gate questions.
**How to apply:** treat both as settled; changes need the maintainer's approval.

Related: [[next-steps]]
