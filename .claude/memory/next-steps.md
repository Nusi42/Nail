---
name: next-steps
description: "Where the project stands and what to do next — read first when continuing in a new session (0.8.8 released 2026-11-21; wave R4 integrated: bulk-action status, stale lists, move picker, drafts safety; next: review, 0.8.9 installer)"
metadata:
  type: project
---

**Where to work:** the maintainer starts Claude Code in the master `<master>`. It is only the launch point: `main` lives there and moves only by `git merge --ff-only work-1`. All edits, builds, tests and commits happen in `<work root>\nail-work-1` (`work-1`, the lead) and `nail-work-2`–`work-7` (packages). See [[work-folders]] (ADR 0132). CodeGraph's index is in `nail-work-1/.codegraph`: pass that folder as `projectPath`.

**Wave R4 done 2026-11-22** (reviews: 15 must-fix findings fixed in `38ed2b96`, ADR 0233, schema v23; verify
gate passed). `release:prepare --version 0.8.9` started for the maintainer's try. Follow-ups: the e2e suite is
load-flaky at 16 workers (green with `--workers=4`); `m1b` `nails_own_draft_copy_continues_its_draft` fails
rarely under load (an older Nail draft copy still listed while its replacement is queued opens as a second draft:
a duplicate, never a loss; pre-existing); bulk Retry / Dismiss cover message writes only; a mass permanent failure
logs one WARN per op; the R4 e2e bulk spec uses 300 messages (plan said 500; core test 2,000).

**Wave R4 integrated 2026-11-21** (ADRs 0230-0232, [[r4-status]], `docs/plan.md` "Wave R4" status): WS1-WS3
rebased onto the lead branch; the core also no longer links a non-`\Draft` message as a draft's server copy; the
`nail-server` lag test checks `OpsChanged` then `Resync`; the `ops` ts-vectors pin `opsWaiting`; Move out of
Drafts sits in the reader toolbar and menus. Release notes "Nail 0.8.9" and `CHANGELOG.md` `[Unreleased]` hold every
R4 item. Package folders 2-4 cleaned. Next: the wave's reviews, then the 0.8.9 installer for the maintainer's human
gate (bulk move / delete of several hundred messages on a real IMAP account), then `release:prepare --version 0.8.9`.

**Wave R4 started 2026-11-21** (nail-wave, packages in folders 2-4, memory r4-status): maintainer report on
0.8.8 after bulk moves/deletes over IMAP: "Saving 14 changes" stuck until restart and stale rows of moved messages
in another folder's list (gone after restart); the diagnostics bundle showed every op done on the server. Fixes:
real queue summary + coalesced ops events + resync refetching lists (no event flood in the 256-slot channel),
deferred replays logged at INFO, no duplicate rows after a UIDVALIDITY reset with pending moves, the Move window
no longer moves on Enter to the first substring match and confirms Drafts/Trash/Junk, non-draft messages in
Drafts open in the reader. Release notes section "Nail 0.8.9". Next: integrate, review, then the 0.8.9 installer.

**0.8.8 released 2026-11-21** (GitHub release `v0.8.8`, `main` published at `f3f00068`).

**0.8.8 prepared 2026-11-21** (`chore(release): release 0.8.8`, all checks passed in prepare, copied to the
releases folder, `0.8.8/x64/Nail_0.8.8_x64-setup.exe`). Holds R1-R3 (0.8.7 was never published). Waiting for the
maintainer: try the installer, then `npm run release:publish -- --yes` (the maintainer runs it, see below). `main`
is not yet pushed to GitHub; publish does that.

**Wave R3 integrated 2026-11-21** (ADRs 0228 pickers multi-value / strict, 0229 rule list multi-select;
[[r3-status]], `docs/plan.md` "Wave R3" status): multi-select rule list with Run / Test on all selected rules, one
Exchange badge per rule, the values button after the value, pickers in every remaining settings field (chips for
several values), the empty address picker lists the most-used addresses, built-in theme Paper. Release notes
"Nail 0.8.8" and `CHANGELOG.md` `[Unreleased]` hold the R3 items. Next: build the 0.8.8 installer for the
maintainer (clean work folder, payload smoke test, releases folder), the maintainer's human gate, then
`release:prepare --version 0.8.8` and publish. Package folders 2-4 were cleaned. Open: README media
(`npm run readme:media`) if not refreshed in the integration (see the plan's status line).

**Wave R3 contract done 2026-11-21** (ADR 0227, `docs/plan.md` "Wave R3"): `SuggestAddresses` with an empty
prefix lists the most-used correspondents (`Store::most_used_recipients`, own addresses left out; mock too); the
rules window decisions (multi-select Run / Test, combined badge, values button place) are fixed in the ADR. 0.8.7
will NOT be published (no tag): the release notes' section is now "Nail 0.8.8", `CHANGELOG.md` holds the entries
under `[Unreleased]` again, the workspace version stays 0.8.7 until `release:prepare --version 0.8.8`. Packages
WR7 (rules list / condition row), WR8 (pickers everywhere), WR9 (Paper theme from the sister project's
`paper.kdl`) start from `main`; the integrator adds the R3 items to the 0.8.8 notes and the changelog. Then
installer 0.8.8 for the maintainer, then release.

**0.8.7 prepared 2026-11-21** (R1 + R2; commit `chore(release): release 0.8.7`, copied to the releases folder;
first prepare run failed on e2e load flakes in m5c / m5d / a11y-windows that passed alone, re-run with
`--skip-checks`). The maintainer tried it; it is not published (superseded by 0.8.8). Maintainer decided 2026-11-21 (next wave R3): one combined Exchange badge per rule row (hover lists every
account); the values-from-mail button right after the text field; multi-select in the rule list with Run running the
selected rules in order on the Run on target; switch every remaining plain dropdown (junk, search index, Sieve,
vacation, MCP grants, import/export, Also my addresses) to the shared picker; the address picker lists the most-used
addresses when empty (core change).

**Wave R1 done 2026-11-20** (rules on demand, live search, shared problem indicator; ADRs 0219-0221;
[[r1-status]]): `main` = 560645e8, published to GitHub. A design review of the rules dialog was published to the
maintainer as an artifact; the maintainer asked for all of it.

**Wave R2 contract done 2026-11-21** (ADR 0222, `docs/plan.md` "Wave R2"): core draft reads, run targets
(schema v21), runs deleted with their rule, `GetMessageFacts`, `SuggestValues`; stubs `widgets/pickers/`,
`features/rules/fromMessage/`, `features/menus/sources.ts`; theme regions. Packages WR3-WR6 start from `main`.

**Wave R2 integrated 2026-11-21** (ADRs 0223-0226, [[r2-status]], `docs/plan.md` "Wave R2" status): redesigned
rules window (footer, Preview / Recent runs, questions, Text view highlighting, 1100x760, stacked below 760 px),
one shared `Picker` (rules editor, virtual accounts, sync, notifications, reply-to, vacation, search values),
values from the mail beside conditions and actions plus "Add to rule…", "Run rule" / "Add to rule…" as hover
submenus, search hints as a floating combobox. Waiting for the maintainer: the human gate on the real app (a new
rule from a mail with values from it, the pickers in the rules editor and the virtual account editor, Run rule as a
submenu, the search hints and values). Deferred items are listed in the plan's status line (TestRule.message_ids,
MessageFacts headers / tags, empty-prefix address browsing, the reader card menu, a drag scenario). Package
folders 2-5 were cleaned after the wave.

**State:** M0-M5, the polish waves P1-P3 and the quality waves Q1-Q2 are done and verified
(status lines in `docs/plan.md`, notes by milestone in `docs/architecture.md`); later changes have their own ADRs
(0180-0201). Version 0.8.0. `main` = `work-1`, pushed to GitHub.

**GitHub:** the repository is public (AGPL-3.0-or-later with the contributor license agreement in
`CONTRIBUTING.md`, ADR 0198). Squash merges only (the pull request title becomes the commit message); the
ruleset on `main` blocks deletion and force pushes and requires linear history, with the maintainer as the only
bypass. Actions is switched off. Pull requests are verified locally before merging (`docs/dev-setup.md`, "Pull
requests"). The remote and the push command are in `CLAUDE.local.md`.

**Waiting for the maintainer:** a lawyer's review of the contributor license agreement before any commercial
license is sold. The installer for 0.8.0 (the newest one in the releases folder predates the license, version and
wording changes) is built on request.

**History (ADRs 0192, 0198, 0200):** rewritten and reworded before the first push; commit ids in older notes and
installer file names no longer exist. Old objects were not pruned everywhere (unreachable objects are never
pushed): the maintainer may run `git reflog expire --expire=now --all && git gc --prune=now` in each folder and
purge OneDrive's version history of the master's `.git`. Personal paths, the remote, the identity, the installer
folder and where the old history is kept: the untracked `CLAUDE.local.md` (ADR 0199).

**Waiting for the maintainer to try** (newest installer in `<releases folder>\`, payload smoke passed): HTML mail
filling the reader (ADR 0193), Show source (ADR 0194), the look-alike sender warning (ADR 0195), folder management
(ADR 0196).

**Wave R1 (rules on demand, live search; 2026-11-20):** integrated (ADRs 0219-0221, [[r1-status]]): run panel in
the rules window, "Run rule" in the row and folder menus, the Editor / Text fix, live search, one shared
`ProblemIndicator`. Waiting for the maintainer: the human gate on the real app (a fresh rule applied to a selection
from the rules window and from the list menu, live search while typing, an invalid query) and one decision: should
the line / column of broken rule text stay visible inline in the text view (now only in the indicator's details)?
Next: the rules window redesign (incl. the misplaced Test rule results) as its own wave. Adding a `.ftl` file may
need the Vitest cache cleared (`node_modules/.vitest-cache`): the catalog glob is cached.

**Released 0.8.6 (2026-11-19):** fixes a user report against 0.8.5 (ADR 0217): MCP `update_draft` /
`send_message` take a draft's message id; an elicitation *decline* (clients that cannot show the form send it at once)
no longer denies sends and deletes, Nail's confirmation decides. Built with `release:prepare` in the lead folder
(output `.nail-build/release/0.8.6/`, commit `chore(release): release 0.8.6`; re-run with `--skip-checks` after the
flaky e2e test below). Published as GitHub release `v0.8.6` (Latest). Release copies now go to the
releases folder by `release:prepare` itself (`NAIL_RELEASES_DIR`, ADR 0218); the old installer folder was moved there.

**Released 0.8.5 (2026-11-19):** GitHub release `v0.8.5` (Latest), `main` published; tried by the maintainer before
publishing. Content: continuing a draft from the Drafts folder (`EditDraftMessage`, `mail.continue-draft`, Outbox
favorite also while drafts exist) and the dependency advisories of ADR 0216 (ammonia, lettre, rmcp updated; chrono /
chrono-tz accepted as unmaintained in `deny.toml`). Dependabot alert #39 (minimatch, dev-only linters) dismissed as
tolerable risk. The visual composer screen now discards its draft (autosave made later screens order-dependent).
Open edge cases of the draft import: non-IMAP draft without Message-ID keeps its original; reply-all drafts from
other clients come back as plain replies; Markdown drafts from other clients import as HTML.

**Open: flaky tests seen in the 0.8.5 release runs** (each passed alone 3–5 times; every prepare run takes about an
hour, so they cost real time): `nail-core` `exchange::shared_mailboxes_sending_and_free_busy` (a removed shared
mailbox's Inbox still exists right after `RemoveExchangeMailbox`: likely a race with a running sync that recreates
the folders, possibly a real core bug: check first); `nail-bench` `a_tiny_run_measures_every_rust_metric`
(cold start 4651 ms over the 4500 ms budget under load); e2e `m2c` right-click menu (list never loaded),
`desktop-health` crashed-page reload (60 s timeout) and (0.8.6 run) `security.spec.ts:382` MCP encrypted grant: the
Settings form re-rendered after `Client name` was filled, so `Create client` stayed disabled (passed 4/4 alone).

**Idle work folders (2026-11-08, published):** a folder with nothing running and nothing waiting is
cleaned with `node scripts/work-folders.mjs clean <n>` (branch to `main`, build output and `node_modules` removed;
refused with uncommitted changes or commits neither in `main` nor pushed). The wave integrator cleans the package
folders after the fast-forward. Folders 2 and 3 were cleaned on 2026-11-08.

**Build speed (2026-11-05, published 2026-11-08):** the scripts pick the cargo job count from the free memory
(ADR 0214); one integration-test binary per crate except `nail-core` (ADR 0213; 206 → 112 binaries, test compile
6:51 → 1:48, workspace `cargo test` about 6.5 min). The release checks run e2e on the test build's `nail`. Measured and
rejected: one release target directory (nothing shared: cargo resolves features per call). Open follow-ups: fix the
three `nail-core` races that merging exposed (ADR 0213; the forced Sieve upload one looks like a real core bug), then
merge its tests; `nail-api` compiles twice per release at about 7.5 min each (crate-local ThinLTO; the MAPI DLL no
longer needs it, ADR 0215, 2026-11-06: cold DLL builds 18 s / 13 s); `cargo doc` takes about 5 min in `npm run check`.

**Released 0.8.4 (2026-11-05):** GitHub release `v0.8.4` (Latest), `main` published. Waits for the
maintainer's check on macOS (links, status bar, text menu over HTML mail). Content: links and the text menu over HTML mail now work in WebKit
(macOS / Linux): every mail frame runs one hash-pinned script in an opaque origin (ADR 0212); review fixes to
0.8.3 (remote desktop-only requests answer `unsupported`, attachment names keep their extension, downloaded files
are marked (Zone.Identifier / `com.apple.quarantine`), PDF path hidden without print-to-PDF, apostrophes in text
links). A Playwright `webkit` project runs the engine-dependent specs on Windows. Verified on the real WebKitGTK
app in WSL under Xvfb (xdotool + screenshots; the app needs a long-lived background `wsl.exe` call). Not compiled:
the macOS `setxattr` code. Known flaky tests, not fixed: the `nail-smime` CBC padding test (about 1 in 256), the
m2c quiet-hours e2e under load.

**Released 0.8.2 (2026-11-01):** security update (zip 9.2 with Nail's own archive checks, rustls,
ammonia and others; ADR 0209) and one update address instead of channels (GitHub's latest release,
`releases/latest/download/latest.json`; ADR 0210). The rolling `stable` / `beta` releases are deleted;
installed 0.8.0 / 0.8.1 copies need one manual install (accepted). `release:prepare` now stops while Dependabot
alerts are open; glib (#1) and minimatch (#34) are dismissed as accepted risks (ADR 0209). The scripts now choose
`CARGO_BUILD_JOBS` from the free memory (ADR 0214); don't set it by hand.

**Released 0.8.1 (2026-10-31):** GitHub release `v0.8.1` (Latest) with the installer, zip, signature,
checksums and SBOMs; the `stable` and `beta` channels announce 0.8.1; the maintainer runs it (installed over
0.8.0, page logging works). Next version, maintainer's wish: the link status bar of the reader (ADR 0206) shows a
mismatched / warned link in warning colours (warning background and border, an icon), and the bar sits flush on
the window's bottom edge (no gap). Logging stays as it is (always on, `info`, 3 × 5 MB; maintainer's decision).

**Latest (2026-10-31, evening), published to GitHub:** links in HTML mail open outside the reader, with the
target in a status bar, a confirmation for warned links and a capped, grouped warnings list (ADR 0206); a crashed or
hung window is logged and reloaded, page errors and memory go to `app.log`, an error screen replaces a blank window
(ADR 0207; the cause of the maintainer's blank window is still unproven, look for `webview page failed` or `page …`
lines in `app.log` next time); remote-content buttons fit a narrow reader. Branches are now rebased, never merged
(ADR 0208, [[rebase-not-merge]]). Released as 0.8.1.

**Latest (2026-10-31): only the master publishes (ADR 0205).** The remote `github` moved from the lead's
work folder to the master; publishing is `git -C <master> push github main` after the fast-forward (exact command in
`CLAUDE.local.md`). The pre-push hook runs `scripts/pre-push.mjs` (decision: `pushPlan` in
`scripts/lib/push-plan.mjs`): work folder → master = guard + `npm run check`; master → public remote (`main` and
release tags only) = guard on the pushed commits and the tracked files + `cargo deny check licenses`; a work folder's
push to a network remote is refused. `release:publish` pushes through the master. The post-edit hook is now a small
loader of the edited checkout's `scripts/lib/after-edit.mjs`, which also strips Claude Code's `modified` /
`node_type` front-matter keys. If the master's hooks ever seem not to run: `git -C <master> config core.hooksPath`
must be `.githooks` (`guard-master` sets it).

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

**Releases (2026-10-25, ADR 0203):** `npm run release:prepare -- --version X.Y.Z` and `npm run release:publish`
(`docs/release.md` "Releasing"). Maintainer decisions: GitHub Releases only (rolling `stable` / `beta` releases hold
`latest.json`), unsigned for now, x64 NSIS + portable zip, first version 0.8.0. The release update key exists; where it
is kept is in the maintainer's `CLAUDE.local.md`, never in the repository (public half
`packaging/updater/release.pub`). 0.8.0 is published (2026-10-26): GitHub release `v0.8.0` (latest)
with the installer, zip, signature, checksums and SBOMs; the rolling `stable` / `beta` releases hold `latest.json`.
Auto mode refuses `release:publish` as a public action: the maintainer runs
`! cd ~/SelfMade.InWork/nail.win1 && npm run release:publish -- --yes` (no stdin there, hence `--yes`). Plug-in budgets now count thread CPU time (ADR 0204).
One unexplained e2e failure during an earlier prepare run: `net::ERR_NO_BUFFER_SPACE` loading the app's script
(Windows socket buffers); not reproduced in the e2e suite or `cargo test` alone (socket peaks about 1,200).

**Possible follow-ups:** the source of events / contacts (iCalendar / vCard) like Show source; EWS folder operations
(CreateFolder / UpdateFolder / MoveFolder / DeleteFolder in the EWS client); MCP / plug-in folder tools.

**Hosted CI is not used** (ADR 0201): the pre-push hook runs `npm run check`; before merging and before a release
run the full local verification (`docs/dev-setup.md` "Verification (no hosted CI)").

**Next steps in order:**

0. **Waiting for the maintainer:** results of the real-account test with installer
   `<releases folder>\Nail_1.0.0-rc.1_2026-10-09_d8491d45-setup.exe` (INBOX stopped at 150 before the
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
   - code signing (SignPath or a standard certificate; EV brings no SmartScreen advantage any more)
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
