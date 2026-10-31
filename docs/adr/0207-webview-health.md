# 0207 — The desktop window stays alive: webview recovery and page diagnostics

Status: accepted (2026-10-31)

## Context

The desktop app ran for about two days with its window hidden (tray). When
the user opened the window, it was white; F5 did nothing; only restarting
Nail helped. `app.log` showed the core healthy all along (a sync every five
minutes, no error, no panic). So the WebView2 page had died, hung or lost
its content, and nothing in Nail noticed or wrote anything about it:

- the shell did not subscribe to WebView2's `ProcessFailed`, so a render
  process that exited or hung left a white webview behind;
- the page had no error boundary and no `error` / `unhandledrejection`
  handler: a render error unmounts the whole React root (a white page) and
  is only written to a console nobody sees;
- F5 never reloads Nail (ADR 0183: it checks for new mail). That runs in
  the page, so a dead or hung page ignores it, and there was no reload of
  Nail's own.

What the machine's records show for the incident (no private data):
WebView2's crash reporter had written no report, Windows Error Reporting
had no fault of `msedgewebview2.exe`, and no display driver reset was
logged; the WebView2 runtime was updated (a new version installed beside
the running one) about 16 hours before the window was opened. So a crash
of the render process is unlikely; a hung or discarded renderer, or a page
whose JavaScript failed, cannot be told apart from what was recorded. That
is the gap this decision closes: the next time, the log says which.

`app.log` itself was checked too: it is rotated at 5 MB with two older
files (ADR 0190); it was 3.8 MB after three weeks, so it had not reached
its first rotation yet.

## Decision

**The shell watches every webview** (`app/src-tauri/src/webview_health.rs`):

- On Windows it subscribes to WebView2's `ProcessFailed` for every webview
  it creates (main window, pop-outs, print windows; from `on_page_load`,
  once per window label, forgotten when the window is destroyed) and logs
  every failure with its kind, reason, exit code and process description.
- A render process that exited or is unresponsive gets its page reloaded,
  after a back-off of 0 s, 5 s, 30 s for the first, second and third
  reload; at most three reloads per 15 minutes. A failure while a reload is
  pending starts no second one. After that — or at once when the browser
  process itself exited, which leaves the webview unusable — Nail gives up
  for that window and says so once in a native message box
  (`shell-window-failed`: quit Nail from the notification area and start it
  again). Frame (mail body), GPU and helper processes are only logged:
  the page lives, or WebView2 restarts them by itself.
- When the main window comes up (`show_main_window`: tray, second start,
  Simple MAPI; and when it gets the focus) the shell pings the page with a
  tiny script (`eval_with_callback`); no answer within 10 s counts as an
  unresponsive renderer (same reload rules). Pings run at most once a
  minute per window. The ping works on every platform; `ProcessFailed` is
  WebView2 only (`cfg(windows)`).
- Recreating the webview or restarting the process automatically was not
  chosen: the main window's label, its pop-outs and the single-instance /
  data-directory locks are bound to the running process, and a restart races
  with both locks. The user's own restart is the documented way out.

**The page reports to `app.log`** (`ReportUiDiagnostics { report: UiReport }`,
answered `Done`; `ui/src/state/pageDiagnostics.ts`, `crates/nail-core/src/ui_log.rs`):

- `started` (the route's first segment only): every reload shows in the log;
- `problem` for uncaught errors (`error`), unhandled rejections and render
  errors caught by the new error boundary around the shell
  (`PageErrorBoundary`, which shows "Something in this window failed to
  show" with **Reload** instead of a white page). At most
  `UI_PROBLEMS_PER_MINUTE` (10) per minute, the rest counted in the next
  one (`suppressed`); the core applies the same cap again, so no page (a
  remote one included) can flood the log, and cuts texts to
  `UI_REPORT_MAX_CHARS`;
- `memory` every `UI_MEMORY_SAMPLE_MINUTES` (30): uptime, hidden or not,
  the JavaScript heap where the engine tells (`performance.memory`, which
  WebView2 has; `measureUserAgentSpecificMemory` needs cross-origin
  isolation and is not used) and the number of elements.

The lines are written at target `nail_ui` (`UI_LOG_TARGET`, also the filter the
e2e tests switch on: their servers log warnings only) and pass the log's redactor like
every other line. The request needs a session, not a grant (no mail data).

**"Reload window"** (`system.reload`, Ctrl+Shift+R, in the palette) is an
action of Nail's own command system, for a page that still runs but shows
nothing. F5 and Ctrl+R keep checking for new mail (ADR 0183); the browser's
reload stays off.

**Leak hunt.** The page was searched for what grows while it runs for days
hidden: timers, listeners, subscriptions, caches, toasts. The query cache
evicts after 5 minutes, listeners and subscriptions are removed, plug-in
toasts and logs, notices and alerts are capped. The PIM toasts (one per
reminder and failed change) were not: they are now capped at 10 through the
shared `pushBounded` (also used by the plug-in toasts). Soak tests with fake
timers run three simulated days (`pageDiagnostics.test.ts`,
`features/pim/toasts.test.ts`). Timers keep running while the page is
hidden (the 30 s clock, the status bar's countdown of a scheduled send);
they cost CPU, not memory, and are left as they are.

## Consequences

- A dead or hung page comes back by itself, and the log tells what
  happened (`webview page failed; reloading it` with the failure, or
  `page started` / `page error` / `page memory` lines from the page).
- A page's unsaved in-memory state is lost on a reload; drafts are saved
  by the core.
- The desktop e2e test `desktop-health.spec.ts` crashes the main window's
  render process (DevTools `Page.crash`) and checks the reload; the shell's
  test control `webview-health` reports each window's reloads.
