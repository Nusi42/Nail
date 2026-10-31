# Memory index

- [Next steps](next-steps.md) — READ FIRST in a new session: current state, what to do next, where the wave arguments are
- [Maintainer phone reporting](owner-phone-reporting.md) — commentable HTML artifact links instead of md files; screenshots to the phone after every milestone
- [No private data in git](no-private-data-in-git.md) — maintainer rule: nothing of the maintainer's accounts / profile in code, tests, docs, memory or commits; generic bug reports to agents; guard patterns outside the repo
- [Commit messages](commit-messages.md) — Conventional Commits in plain English; never Claude-Session lines or claude.ai links; no internal jargon (ADR 0200)
- [SSOT / ODR / DRY](ssot-odr-dry.md) — mandatory maintainer rule for all code; in CLAUDE.md + wave workflow; reviews flag violations as must-fix
- [Installer per wave](owner-installer-per-wave.md) — after every merged wave: NSIS setup built in nail-work-1, smoke-tested, copied to the maintainer's installer folder (CLAUDE.local.md)
- [Work folders](work-folders.md) — master checkout (git only, NAIL_CHECKOUT_ROLE=master) + numbered work folders and branches (ADRs 0132, 0199); concrete paths only in CLAUDE.local.md
- [Rebase, never merge](rebase-not-merge.md) — branches are rebased onto the lead's branch; pre-push refuses merge commits (ADR 0208)
- [Backlog: real-account tests](backlog-real-account-tests.md) — Google/Exchange real-service tests parked in docs/backlog.md
- [Zero-warnings policy](zero-warnings-policy.md) — linters/formatters for all files, 0 warnings/0 errors, never disable good lints; expect-with-reason only
- [Maintainer UI decisions](owner-ui-decisions.md) — no fit-to-width for wide HTML mail (scrollbar is fine); accent override ignored in High contrast (ADR 0161)
