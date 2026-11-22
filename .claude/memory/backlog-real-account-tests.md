---
name: backlog-real-account-tests
description: Real Google/Microsoft account and real Exchange/EWS server tests are deliberately parked in docs/backlog.md (project decision)
metadata:
  type: project
---

Google and Exchange real-account / real-server testing is parked in the backlog (`docs/backlog.md`). Until then the
automated suites rely on mock servers and recorded fixtures.

**Why:** these tests need real accounts / tenants the project does not have yet, and they do not block development.
**How to apply:** don't ask for Google/Microsoft test accounts or grommunio/Exchange labs unless the maintainer brings
it up; when a milestone needs real-service sign-off, point to the backlog entries. Claude never creates third-party
accounts on anyone's behalf.

Related: [[r4-status]]
