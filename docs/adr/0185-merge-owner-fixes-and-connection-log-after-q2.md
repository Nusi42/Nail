# ADR 0185: Desktop-test fixes and the connection log merged after Q2 — pinned demo "now", isolated WebView2 profiles

Status: accepted · 2026-10-08 · merge of `work-6` (ADRs 0180–0184) and `work-7` (ADRs 0190, 0191) into `work-1`
after wave Q2 (ADRs 0171–0177)

## Context

`work-6` (fixes from the maintainer's desktop test: IMAP first, windows follow the app, text selection, Check for new mail, Nail's own context
menus) and `work-7` (connection log, error details, app log, resilient IMAP folder sync; crate `nail-connlog`,
schema v19) branched from `ee62b66`, before Q2 moved shared code into `nail-net`, `nail-util`, generated
constants and shared test helpers and made the duplicate check blocking. Two leftovers were open: the visual
`calendar-year` screens drifted with the date (the demo-multi feeds and CalDAV events were dated from the real
`Utc::now()`), and a desktop e2e run once collided with the maintainer's installed Nail (WebView2 `0x8007139F`, a
profile held by another process with other options).

## Decisions

### Merge resolutions (every behaviour of the three lines kept)

- **Line-protocol clients.** work-7's connection taps sit on Q2's `nail-net` plumbing: IMAP / POP3 / ManageSieve
  take the TLS stream from `nail_net::tls::connect` and then log the session (`ConnTap::tls_session`); IMAP's
  pre-auth line reader is `nail_net::io::read_line_limited` plus the tap; the IMAP connection's oversized-literal
  limit is `nail_net::io::MAX_MESSAGE` (1 MiB under test, ADR 0191).
- **HTTP clients: one logged request path.** The tap of the Gmail / Graph clients moved into Q2's shared
  `nail_net::api::ApiClient` (`with_tap`, also through `api_client_wrapper!`) instead of two copies of work-7's
  request loop. `nail_net::http::send_logged` (send, bounded body, log the answer or the failure) serves the API
  clients and the DAV client; `hide_credential` keeps the credential of every `Authorization` value out of the
  log (API clients, DAV, EWS). `nail-net`'s `http` feature depends on `nail-connlog`.
- **One redactor placeholder.** `nail_api::connlog::REDACTED` re-exports `nail_util::redact::REDACTED`; the
  diagnostics bundle uses `nail-connlog`'s rules (work-7) — Q2's copy of the secret keys in `nail-portability` is
  gone.
- **Engine failures.** Q2's `report_offline` / `report_sync_failure` are replaced by work-7's `engine_failed`
  (status, stored error, `SyncFailed` with details); `protocol_failed` is its shorthand with the details read
  from the error. An engine that cannot start now shows the state its error means (work-7), not always Offline.
- **Request groups.** `nail_api::connlog_admin_requests!` (Q2 style) names the connection-log / app-log
  requests once for dispatch and remote access.
- **Generated lists.** `CONTEXT_MENU_PARTS` stays generated from `nail_api::menus` (work-6's `folder-tree` and
  `text` are there); the connection log's enums come from `ts_constants.rs` (`WIRE_PROTOCOL_VALUES`,
  `WIRE_LEVEL_VALUES`, `WIRE_DIRECTION_VALUES`, `ERROR_STEP_VALUES`) instead of hand-written `satisfies` sets.
- **Shared UI / test homes.** The connection-log skin uses `theme/tokens.ts`, `theme/selectors.ts`,
  `ACCENT_BAR` and the new `FIELD_BASIS` (also portability's); repeated Fluent texts refer to `common.ftl`
  (new: `common-copy`, `-protocol`, `-connection`, `-managesieve`, `-note`, `-error`, `-export-ellipsis`,
  `-text-files`); the window sweeps share `connlogStep` (`e2e/support/windowTour.ts`, connection-log windows in
  `FEATURE_DIALOGS`); Vitest shares `BUILTIN_THEME_IDS` and the connlog `testUtil.ts`; the store reads rows with
  `collect_rows`. Exception added to `scripts/duplicates/allow.json`: `MAX_DEPTH = 64` (XML nesting vs IMAP
  parenthesis nesting, coincidental).
- ADR numbers 0180–0184 and 0190–0191 stay (no collision with Q2's 0171–0177); schema v19 stays (Q2 added no
  migration).

### A pinned demo "now" (leftover a)

- `NAIL_DEMO_NOW` (RFC 3339; `nail_core::demo::DEMO_NOW_VAR`, read only by the demo seeds): the demo-multi
  feeds and CalDAV events are dated from its day, and the demo clock (`AdjustableClock`: snooze, reminders,
  plug-in jobs) starts at it and runs on. Unset, both use the real time as before (manual demos and the e2e
  specs that work relative to today are unchanged).
- The visual suite starts its servers with `demoNow: MAIN_TIME` (`StartOptions.demoNow` in
  `e2e/support/nail.ts`), the same instant its pages' clock shows: the year view shows the demo's October
  events on the same days in every run. The Exchange spec already pinned both of its clocks (the mock tenants'
  items are dated absolutely).

### Every desktop test launch has its own WebView2 profile (leftover b)

- Q2 (ADR 0177) gave seeded runs on given directories `<data-dir>/webview`. In addition, the e2e harness
  (`startDesktop`) sets `WEBVIEW2_USER_DATA_FOLDER` to a fresh temporary folder for every launch, so a window
  that is ever created without its own folder still never reaches WebView2's default profile — the one an
  installed Nail holds.

## Consequences

- `npm run check` (duplicate check `--strict`) is 0 findings after the merge.
- Specs that need fixed calendar content start their server with `demoNow`; they must pin the page's clock to
  the same instant (`page.clock.setFixedTime`).
- The calendar-year baselines were refreshed once with the pinned date.
