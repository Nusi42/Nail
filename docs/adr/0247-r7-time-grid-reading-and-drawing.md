# 0247 — Time grid: cascaded overlaps and drawing new events (R7)

Status: accepted (2026-11-24)

## Context

ADR 0246 fixed what the R7 time grid shows (M2, M3, M5, M6, S1, S4, S6,
S12) but left the exact rules to the package: when two overlapping events
cascade and when they stand side by side, what a click on empty time does,
how the keyboard draws a range, and where a new event goes without the
editor.

## Decision

- **Cascade levels (`time.ts` `placeDay`).** An event is one level above
  every overlapping event that started at least `SIDE_BY_SIDE_MINUTES` (15,
  one grid step) before it, and on the same level as overlapping events that
  started within that step (their titles would cover each other in a
  cascade, so they stand side by side in greedy columns). Each level is
  indented by `CASCADE_INDENT` (a fifth of the column), less when there are
  many levels so the top card keeps `CASCADE_MIN_WIDTH` (60 %); every card
  reaches the column's right edge, and a side-by-side card expands into the
  columns to its right that nothing overlapping uses. A card's level is its
  stacking order (`z-index` level + 1); cards above level 0 carry the state
  `stacked` (the outline in the page colour).
- **Short cards:** shown length under `SHORT_EVENT_MINUTES` (45).
- **Drawing (`dragRange`, `clickRange`):** a press on empty slots draws from
  the 15-minute step it started in to the pointer snapped to the nearest
  step, at least one step, in either direction; after
  `DRAW_THRESHOLD` (4 px) of travel it is a drag, else a click, which takes
  the step under the pointer and `DEFAULT_EVENT_MINUTES`. Both end in the
  inline title (`NewEventDraft.tsx`, the shared `Popover`). A double-click
  on empty time opens the editor at that step; the keyboard's
  Shift + ↑ / ↓ extend the slot grid's range by an hour, Enter asks for the
  title of the range (before R7 Enter opened the editor directly).
- **Where a new event goes without the editor:** `newEvent.ts`
  `defaultEventCalendar` — the preferred calendar when writable, else the
  first local one, else the first writable one — the same rule as the
  editor's preselection; `createEventIn` saves the title and times with the
  calendar's default reminders. A task dropped on the grid uses the same
  path with the task's title.
- **Event clicks:** `EventItem` opens the quick look on a click (and Enter
  / Space, which click a button) and the editor on a double-click, in every
  view that shows cards.
- **Icons:** repeats show `repeat` (bundled for this at integration, with
  `video` for the editor's call link chip); people `users`, reminders `bell`, read-only `lock`, an
  unanswered invitation `circle-question-mark`. A theme may replace any of
  them by name.
- **Working time:** working days get hatched stretches outside the working
  hours, days off (not in `work_days`) a tint over the whole day and their
  header. The grid scrolls to the working day's start unless the widget sets
  `scroll-to`.
- **Today:** slots no longer carry the state `today`, so the theme's
  `calendar.slot:today` shade no longer applies (the rule in
  `defaults/theme.kdl` is left for the integrator to remove).

## Consequences

- A double-click is the way to the editor from a card; tests that clicked
  a card for the editor double-click it.
- `newEvent.ts` holds the default-calendar rule; the editor and the
  toolbar's quick add use it rather than a copy.
