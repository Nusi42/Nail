# Memory index

- [No private data in git](no-private-data-in-git.md) — maintainer rule: nothing of the maintainer's accounts / profile in code, tests, docs, memory or commits; generic bug reports to agents; guard patterns outside the repo
- [Public repository, no personal setup](public-repo-no-personal-setup.md) — tracked files describe only what anyone who clones Nail needs; personal development setup stays in untracked local files (ADR 0234)
- [Commit messages](commit-messages.md) — Conventional Commits in plain English; never Claude-Session lines or claude.ai links; no internal jargon (ADR 0200)
- [SSOT / ODR / DRY](ssot-odr-dry.md) — mandatory maintainer rule for all code; in CLAUDE.md + wave workflow; reviews flag violations as must-fix
- [Work folders](work-folders.md) — optional master checkout (git only, NAIL_CHECKOUT_ROLE=master) + numbered work folders and branches (ADRs 0132, 0199); local paths only in an untracked CLAUDE.local.md
- [Rebase, never merge](rebase-not-merge.md) — branches are rebased onto the lead's branch; pre-push refuses merge commits (ADR 0208)
- [Backlog: real-account tests](backlog-real-account-tests.md) — Google/Exchange real-service tests parked in docs/backlog.md
- [Zero-warnings policy](zero-warnings-policy.md) — linters/formatters for all files, 0 warnings/0 errors, never disable good lints; expect-with-reason only
- [Maintainer UI decisions](owner-ui-decisions.md) — no fit-to-width for wide HTML mail (scrollbar is fine); accent override ignored in High contrast (ADR 0161)
- [R1 status](r1-status.md) — rules on demand + live search integrated (ADRs 0219-0221); shared ProblemIndicator; layout snapshot pitfalls
- [R2 status](r2-status.md) — rules window redesign, shared pickers, values from the mail, menu sources integrated (ADRs 0222-0226); one picker / menu button; fixture and golden regeneration pitfalls
- [R3 status](r3-status.md) — rule list multi-select, combined Exchange badge, pickers everywhere (multi-value, strict), Paper theme (ADRs 0227-0229); theme CSS subset, stale Vitest glob cache
- [R4 status](r4-status.md) — bulk actions: queue summary + resync, no OpStatus for message writes, combobox Move window, drafts safety (ADRs 0230-0232); snapshot / vitest-cache pitfalls
