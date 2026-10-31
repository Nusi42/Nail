---
name: owner-phone-reporting
description: "The maintainer follows the work mostly on the phone — report via commentable HTML artifact links, send screenshots after every milestone"
metadata:
  type: feedback
---

Report proposals, status and results as an HTML artifact link the owner can comment on, never as local `.md` files to read. After every milestone (and at other natural checkpoints) send screenshots of the running app to the owner's phone (SendUserFile) as proof that development continues.

Screenshot sets must mix light and dark themes (e.g. Frost and Graphite / Nocturne), never light only (requirement).

**Why:** the maintainer follows the work mostly on the phone.
**How to apply:** publish reviews/plans/milestone summaries with the Artifact tool and reply with the link; after a milestone gate, capture screenshots (e.g. the visual-regression screens or a `nail serve --seed demo` run) and send them. Keep chat replies short.

Related: [[next-steps]]
