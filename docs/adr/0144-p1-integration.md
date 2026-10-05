# ADR 0144: P1 integration — responsive layout, HTML mail, stability

Status: accepted · 2026-10-05 · P1 WP194 (integration; contract ADR 0139)

## Context

The four P1 packages were merged into `work-1` without conflicts: WP190 (HTML mail
and the mail view, ADR 0140), WP191 (window manager, windows, dialogs and the
settings page, ADR 0141), WP192 (calendar, contacts, tasks, nav rail and the M5d
editors plus the M5 cosmetic list, ADR 0142) and WP193 (tests flaky under load,
ADR 0143). All four package ADRs had taken 0140; they are numbered 0140–0143 in
package order. Their reports asked the integrator for a few changes across
package boundaries.

## Decisions

- **Fixed pane sizes shrink to their min-sizes.** `flexForSize` in
  `ui/src/layout/engine.ts` returns `0 1 <size>` for fixed sizes (px, %) instead
  of `0 0 <size>`. A split with a fixed side pane no longer pushes its flexible
  neighbour past the window's edge when the window is narrower than the sum of
  the sizes and min-sizes (WP192 saw this in the calendar at 800 px); the
  pane's `min-sizes` still bound it (`useGeometry`). WP192's targeted skin rule
  for `calendar-panes` is removed.
- **Desktop-only cards say so in the browser.** In `nail serve` and the remote
  UI the Updates, Tray and startup and Default apps cards showed only their
  titles (WP191 follow-up). `features/system/QueryProblem.tsx` renders a hint
  ("This setting is available in the Nail desktop app.", `system-desktop-only`)
  for an `unsupported` answer and an error for anything else; those queries do
  not retry `unsupported`.
- **One button row in the diagnostics dialog.** The dialog had three button rows
  (preview, save, close); only the last stays at the window's bottom
  (`DIALOG_BUTTON_ROWS`). The three buttons now share one row at the end.
- **Window clamping.** WP191's window manager keeps a window that was placed
  on the page inside it when the app shrinks, which also covers WP192's request
  (a theme editor opened at 1600 px stays reachable at 800 px).
  `p1-views.spec.ts` still reopens its windows per size; that is kept as it
  is (it also checks the windows' opening sizes).
- **Deferred:** moving the sideways-scrolling-pane and sticky-row checks of
  `p1-windows.spec.ts` into `e2e/support/responsive.ts`; hiding inline images
  that the body shows from the attachment cards (needs the core to report the
  embedded parts: a protocol change, out of P1's scope); the accent override
  note in High contrast (WP192's recommendation, for the owner to decide); a
  larger composer min height; a token for the free/busy name column; a P1
  sweep in compact density and High contrast.

## Consequences

- Layout widgets with a fixed size may now get narrower than their size in a
  window that is too small for all panes; themes and layouts that need a hard
  size set `min-sizes`.
- New dialogs must be listed in `p1-windows.spec.ts` or another P1 spec
  (coverage test), as before for the a11y window sweep.
