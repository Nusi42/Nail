# ADR 0091 (WP114): Desktop settings UI — updates and About, tray and startup, default apps, first run, drafts from other programs

Status: accepted · 2026-10-02 · M4b WP114 (number to be assigned by the integrator) · amended by ADR 0093
("Start with Windows" locked with a note when the machine's Run key manages it)

## Context

ADR 0087 fixed the M4b contract: 12 desktop-only requests (installation
facts, update settings / state / commands, tray and startup settings,
default apps, first run), 3 events and `LaunchRequest::ComposeDraft` for
Simple MAPI "Send to". WP114 builds the UI in `ui/src/features/system/`
against it, with the mock core simulating the desktop host, while WP111–
WP113 build the DLL, installers and shell in parallel.

## Decisions

- **Widgets, not one page.** Three layout widgets matching the kinds the
  contract registered in `nail-config/src/widgets/system.rs`:
  `update-settings` (version line, update status panel, channel, automatic
  check / download; option `show-installation`), `desktop-settings` (close
  to tray, start with Windows, start hidden — disabled unless Nail starts
  with Windows —, the `nail plugin dev` endpoint with its URL while it
  runs; option `show-dev-endpoint`), `default-apps` (one row per target
  plus the Simple MAPI client, "Choose in Default apps", "Check again";
  option `show-mapi`). The default layout places them as settings tabs
  (WP115).
- **"About Nail" and the first-run screen are dialogs** (`about-nail`,
  `first-run`; titles `window-about-nail` / `window-first-run`), opened
  by actions `system.about` / `system.first-run`. Fixed-purpose windows,
  so dialogs rather than layout windows (features README).
- **One update status panel** (`UpdateStatusPanel`) for the widget and
  "About": the state text in a polite live region, a `<progress>` for
  downloads (percent when the size is known, MB otherwise), "Download"
  (only in `available`, i.e. without automatic download), "Restart to
  install" (only `ready-to-install`), "Check now" / "Try again" (disabled
  unless `can_check`). The disabled state shows the contract's reason text
  and no update buttons; a forced channel locks the select with
  `update-channel-locked`. Buttons follow `canCheck` / `canDownload` /
  `canInstall`, so the UI never sends a request the core would refuse
  with `conflict`; when the core refuses anyway (policy changed), its
  localized reason is shown.
- **Release notes are text.** They are untrusted Markdown (ADR 0087).
  `parseReleaseNotes` reads headings, bullet lists and paragraphs and
  strips inline markers and link targets; React renders the result as
  text nodes. No Markdown library, no HTML, no links to click.
- **Default apps are re-read on focus.** Windows does not announce changes
  made in Settings, so the query refetches when the window gets the focus
  back (`refetchOnWindowFocus: "always"`), besides `DefaultAppsChanged`.
- **First run is offered once, desktop only.** The effect `FirstRunGate`
  opens the dialog when `GetInstallation` succeeds (so not in `nail
  serve`, the e2e harness or remote sessions) and `GetFirstRun` says not
  completed; once per session. "Get started" and "Skip" both call
  `CompleteFirstRun`; closing with × leaves it for the next start. Its
  steps: default apps (current `mailto:` state + button), start with
  Windows (checking it also sets "start hidden", as the design says
  "minimized to tray"), and the import offer. The import step shows a
  placeholder until an action `import.open` exists (M4c WP124 registers
  it; the first-run screen then completes and runs it).
- **Drafts from other programs.** `openProgramDraft(ctx, request)`
  remembers the program per draft (feature store keyed by the UI store)
  and opens the draft with `openRichDraft`; `ProgramDraftNotice`
  (mounted in the composer) shows `mapi-draft-from-program` (or
  `system-program-draft-unknown`) above the draft, dismissible. The
  composer already shows the prefilled recipients, subject, text and
  attachments from the saved draft. Nothing is sent automatically.
- **Mock = the desktop host.** `mock.ts` simulates the updater like the
  shell (check → available / up to date; automatic or manual download in
  steps with progress; ready to install; installing), an update server
  per channel (`releases`), failures (`failNext`), `setUpdateState` to
  move it by hand (it cancels pending steps), `setDefaultApps`, and
  `simulateMapiSendMail` = `Service::submit_mapi` (checks, files, plain
  draft from the default identity, `ComposeDraft` launch request; MAPI
  result codes). The mock starts as a returning user (first run
  completed) so the first-run dialog does not cover every mock view;
  tests set it back.
- **Theme base CSS** `SYSTEM_BASE_CSS` (plain lists, wrapping button rows,
  release notes wrapping); colours via `data-state` / `data-tone`.

## Consequences

- WP115 mounts `ProgramDraftNotice` in the composer, calls
  `openProgramDraft` from `features/launch` for `ComposeDraft`, appends
  `SYSTEM_BASE_CSS`, places the widgets as settings tabs and adds the
  actions to menus; nail-config gets the three widget options typed.
- German texts for `system.ftl` come later (empty file; English
  fallback).
