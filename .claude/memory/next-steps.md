---
name: next-steps
description: "Where the project stands and what to do next — read first when continuing in a new session (README, AGPL-3.0, 0.8.0, setup out of the repo; ADRs 0197–0199; history rewrite + first push pending)"
metadata:
  node_type: memory
  type: project
  modified: 2026-10-17T12:29:46.656Z
---

**Where to work:** the maintainer starts Claude Code in the master `<master>`. It is only the launch point: `main` lives there and moves only by `git merge --ff-only work-1`. All edits, builds, tests and commits happen in `<work root>\nail-work-1` (`work-1`, the lead) and `nail-work-2`–`work-7` (packages). See [[work-folders]] (ADR 0132). CodeGraph's index is in `nail-work-1/.codegraph`: pass that folder as `projectPath`.

**State:** M0-M5, the polish waves P1-P3 and the quality waves Q1-Q2 are done and verified
(status lines in `docs/plan.md`, notes by milestone in `docs/architecture.md`); later changes have their own ADRs
(0180-0200). Version 0.8.0 (ADR 0198). `main` = `work-1`, pushed to GitHub.

**History (ADRs 0192, 0198, 0200):** rewritten and reworded before the first push; commit ids in older notes and
installer file names no longer exist. Old objects were not pruned everywhere (unreachable objects are never
pushed): the maintainer may run `git reflog expire --expire=now --all && git gc --prune=now` in each folder and
purge OneDrive's version history of the master's `.git`. Personal paths, the remote, the identity, the installer
folder and where the old history is kept: the untracked `CLAUDE.local.md` (ADR 0199).

**Waiting for the maintainer to try** (newest installer in `<installer folder>\`, payload smoke passed): HTML mail
filling the reader (ADR 0193), Show source (ADR 0194), the look-alike sender warning (ADR 0195), folder management
(ADR 0196).

**Possible follow-ups:** the source of events / contacts (iCalendar / vCard) like Show source; EWS folder operations
(CreateFolder / UpdateFolder / MoveFolder / DeleteFolder in the EWS client); MCP / plug-in folder tools.

**Running:** the GitHub CI run of the latest push: check its jobs and fix runner differences
(release checklist item 5).

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
   - first green GitHub CI run (see Running)
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
