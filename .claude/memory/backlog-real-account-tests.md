---
name: backlog-real-account-tests
description: Real Google/Microsoft account and real Exchange/EWS server tests are deliberately parked in docs/backlog.md (maintainer decision)
metadata:
  type: project
---

The maintainer decided to put Google and Exchange real-account / real-server testing into the backlog (`docs/backlog.md`) instead of doing it now. Until then the automated suites rely on mock servers and recorded fixtures.

**Why:** setting up accounts/tenants needs the maintainer (phone verification, terms, possible costs) and is not blocking development.
**How to apply:** don't prompt the maintainer to create Google/Microsoft test accounts or add grommunio/Exchange labs unless they bring it up; when a milestone needs real-service sign-off, point to the backlog entries. Claude should not create third-party accounts on the maintainer's behalf.

Related: [[next-steps]]
