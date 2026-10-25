---
name: next-steps
description: Where the project stands and what to do next — read first when continuing in a new session (0.8.0 public; Linux build, platform gating, narrow-window open and flaky-test fixes committed on the lead branch, not yet on main)
metadata:
  node_type: memory
  type: project
  modified: 2026-10-25T17:00:00.000Z
---

**Where to work:** the maintainer starts Claude Code in the master `<master>`. It is only the launch point: `main` lives there and moves only by `git merge --ff-only work-1`. All edits, builds, tests and commits happen in `<work root>\nail-work-1` (`work-1`, the lead) and `nail-work-2`–`work-7` (packages). See [[work-folders]] (ADR 0132). CodeGraph's index is in `nail-work-1/.codegraph`: pass that folder as `projectPath`.

**State:** M0-M5, the polish waves P1-P3 and the quality waves Q1-Q2 are done and verified
(status lines in `docs/plan.md`, notes by milestone in `docs/architecture.md`); later changes have their own ADRs
(0180-0201). Version 0.8.0. `main` = `work-1`, pushed to GitHub.

**GitHub:** the repository is public (AGPL-3.0-or-later with the contributor license agreement in
`CONTRIBUTING.md`, ADR 0198). Squash merges only (the pull request title becomes the commit message); the
ruleset on `main` blocks deletion and force pushes and requires linear history, with the maintainer as the only
bypass. Actions is switched off. Pull requests are verified locally before merging (`docs/dev-setup.md`, "Pull
requests"). The remote and the push command are in `CLAUDE.local.md`.

**Waiting for the maintainer:** a lawyer's review of the contributor license agreement before any commercial
license is sold. The installer for 0.8.0 (the newest one in the installer folder predates the license, version and
wording changes) is built on request.

**History (ADRs 0192, 0198, 0200):** rewritten and reworded before the first push; commit ids in older notes and
installer file names no longer exist. Old objects were not pruned everywhere (unreachable objects are never
pushed): the maintainer may run `git reflog expire --expire=now --all && git gc --prune=now` in each folder and
purge OneDrive's version history of the master's `.git`. Personal paths, the remote, the identity, the installer
folder and where the old history is kept: the untracked `CLAUDE.local.md` (ADR 0199).

**Waiting for the maintainer to try** (newest installer in `<installer folder>\`, payload smoke passed): HTML mail
filling the reader (ADR 0193), Show source (ADR 0194), the look-alike sender warning (ADR 0195), folder management
(ADR 0196).

**Latest (2026-10-22):** a user reported that Claude Code refused Nail's `tools/list` ("ttlMs: expected number,
cacheScope: expected one of public | private"): it validates list results against the 2026-07-28 schema even on a
2025-11-25 session, and rmcp 3.5.0 leaves both fields out. Fixed and published (`af95bf28`): every list and
`resources/read` result carries `ttlMs: 0` / `cacheScope: "private"` (`uncached!` in `crates/nail-mcp/src/server.rs`,
test `results_carry_caching_hints`). The reporter can drop their stdio adapter once they run a build from this
`main`; no installer was built. The lead folder was set up fresh (`npm ci`, `npm run build`, `github` remote).

**Latest (2026-10-25): Linux build and platform gating (ADR 0202)**, committed on the lead branch; push to
`main` / GitHub waits for the maintainer's go-ahead. Nail builds, lints and tests on Linux (WSL2 distro
`nail-ubuntu`, setup in `docs/dev-setup.md` "Linux (WSL2) and macOS"; checks run in a clone on the distro's own
disk). The core reports its OS and platform-only features with `GetConfig`; the UI hides Windows-only settings on
macOS / Linux; texts select on `$os`. Reviewed with a fresh context; all must-fix findings fixed. Verified: Windows
`npm run check`, `npm test`, `cargo test --workspace`, full Playwright; Linux clippy, `cargo test --workspace`,
`npm test`, the platform scenario. Running WSL next to Windows builds ran the machine out of memory: run them one
after the other (`wsl --shutdown` frees the VM). Open follow-ups (ADR 0202): native file dialog and PDF printing
off Windows, LAN address list, S/MIME trust anchors off Windows; optional review notes: a test pinning
`ListWindowsCertificates` / SSPI to `PlatformFeature::IN_CORE`, a direct test of the gated Simple MAPI row.

**Then (2026-10-25, committed on the lead branch after it):** the demo tenant's automatic replies are scheduled
around today (`Tenant::schedule_replies_around`); a narrow window switches to the reader tab on double-click / Enter
/ o (`revealWidgets` in the UI store, `conversation.open-in-reader`); three flaky tests fixed — two were real bugs:
a sync deleted a message moved locally whose move still waited (Exchange listings now skip messages with a waiting
op, `has_waiting_op` / `WAITING_STATES` in `nail-store` queue.rs), and a meeting answer kept the event's old etag so
the next sync dropped a change queued after it as a conflict (`respond_to_meeting` now returns the new change key);
the IDLE test waited for its announcement wrongly (`recv_event` in the core tests' common module skips lag). A
proptest found a tokenizer bug (combining marks after a tab made a token with whitespace), fixed.

**Load-sensitive tests (fixed, 2026-10-25):** four Exchange tests that timed out under extreme CPU load (WSL, 96
busy loops on 32 cores) had real causes: a Graph folder was stored before its Exchange record, so a rename right
after its creation was refused as a system folder (`upsert_exchange_folder` now stores both in one transaction);
Gmail and Exchange stored each new message in its own transaction (`ingest_api_messages` now stores a chunk of 25
at once: the 520-message listings take half the time); and a sync reset the user's own meeting answer to "not
answered" (Graph and EWS report it in the event's own response, not the attendee list), so the next edit answered
again (`event_from_exchange` takes the user's addresses). 10 of 10 runs of the Exchange tests pass under that load.

**Possible follow-ups:** the source of events / contacts (iCalendar / vCard) like Show source; EWS folder operations
(CreateFolder / UpdateFolder / MoveFolder / DeleteFolder in the EWS client); MCP / plug-in folder tools.

**Hosted CI is not used** (ADR 0201): the pre-push hook runs `npm run check`; before merging and before a release
run the full local verification (`docs/dev-setup.md` "Verification (no hosted CI)").

**Next steps in order:**

0. **Waiting for the maintainer:** results of the real-account test with installer
   `<installer folder>\Nail_1.0.0-rc.1_2026-10-09_d8491d45-setup.exe` (INBOX stopped at 150 before the
   resilient sync, ADR 0191). If it still fails, ask for "Show details → Copy details" and the connection log lines
   (Warnings and errors) before the error, then fix it in a new package. Nothing is running in the background.
1. The maintainer tests the newest installer: Check for new mail (button, F5 / Ctrl+R), right-click menus (folder pane,
   text fields), windows following the app size, text selection, IMAP first in account setup, Settings → Accounts →
   Connection (connection log, Recent problems, Show details), Data & backup → application log.
2. The maintainer's human gate for P3: conversation actions with undo in the unified inbox (Archive / Delete / labels on Gmail), long HTML mail in the card feed (desktop WebView2 and the remote UI on a phone).
3. The maintainer's human gate on the real app (desktop + `nail serve`): conversation rows, the card feed (HTML mail with inline images), `threads="expandable"`, Settings → Appearance in High contrast, the P1 resize checks (resize windows, open HTML mail). Owner decisions taken: [[owner-ui-decisions]] (no fit-to-width for wide HTML mail; accent ignored in High contrast).
4. Follow-ups if the maintainer wants them: the deferred items in the `docs/plan.md` status lines of Q2, P3 (ADR 0169) and P1 (ADR 0144: shared-helper checks from `p1-windows.spec.ts`, composer min height, free/busy column token).
5. Release checklist (`docs/release-checklist.md`):
   - full local verification run (item 5)
   - security review sign-off
   - EV certificate
   - update key
   - the encryption-at-rest default decision
   - real-account tests from `docs/backlog.md`, as far as the maintainer wants

**Housekeeping:**

- Disk C: runs low. After every wave that is green and merged, delete the build output: `target/` and `.nail-build/` in
  `nail-work-2`–`work-6` (check first that each branch head is in `origin/main`), and in `nail-work-1` the `.nail-build/`
  verify / review / package target dirs and `target/debug/incremental`. Keep `.codegraph`, memory, `node_modules`
  and `.nail-build/screens` (requirement). Last cleanup: work-2–work-7 + work-1 scratch; C: ~2,780 GB free.
- The dev update key lives in `%LOCALAPPDATA%\nail-dev\keys` (`NAIL_KEYS_DIR` overrides).

**How waves run:** see `.claude/workflows/README.md`. The Workflow tool takes the script inline or from a path it can read; the session's folder is the master, so the script path is the master's `.claude/workflows/nail-wave.js`. Owner preferences for every wave are in memory: work folders, zero warnings, no fixed dev ports, `cargo -j 4` on the shared machine, never kill processes you did not start.

**Resume rule:** a session may stop after any step, and a fresh one continues with
"Continue where we left off: follow 'If you are the lead' in CLAUDE.md and .claude/memory/next-steps.md." So after
EVERY finished step (wave, package, merge, installer, cleanup, owner decision): update this file (state, what is
running, what waits for the owner, next steps), commit it on `work-1` and fast-forward `main` before reporting. When work
starts in the background, record it here too (workflow / agent, folder, branch), so a fresh session can check
`git log main..origin/work-<N>` and finish or merge it.

**Why:** a new session should continue without the old conversation.
**How to apply:** read this first, check `git log` in `nail-work-1`, `git log main..origin/work-<N>` for every package
branch, and the `docs/plan.md` status lines, then continue with the first unfinished step.

Related: [[work-folders]], [[owner-installer-per-wave]], [[owner-ui-decisions]], [[backlog-real-account-tests]], [[commit-messages]], [[no-private-data-in-git]]
