---
name: r7-status
description: "Wave R7 (calendar that reads well; release 0.8.11): what was integrated (ADRs 0246-0250) and fixed after review, the shared rules to use and the pitfalls met"
metadata:
  type: project
---

**Integrated (ADR 0246 contract, 0247 time grid, 0248 quick look and editor, 0249 views and shell, 0250 core),
release notes "Nail 0.8.11":**

- A click (Enter / Space) on an event opens the quick look (`setQuickLook` with `anchorOf(card)`), a double-click
  the editor (`useOpenInstance` in `EventItem.tsx`). E2e and tours that want the editor double-click the event or
  press Edit in the quick look; the agenda entries do the same.
- One rule each, use them instead of copies: the calendar of a new event `newEvent.ts` `defaultEventCalendar`
  (preferred, else this device's, else the first writable; editor, quick add, drawn events); hiding declined
  occurrences `time.ts` `visibleInstances(list, prefs.show_declined)`; the user's own answer
  `EventFields::my_status` (core); default colours `Store::save_calendar` + `next_calendar_color`.
- The week start preference (`weekStartOf(prefs)`) goes through `ViewOptions.weekStart`, `monthGrid(month, locale,
  first)`, `dayGridMove(..., { weekStart })` and `MonthGrid` `weekStart`; `startOfWeekFor(day, locale, first)` is
  the one helper. View zones come from `useCalendarZones(node)` (layout option, then preferences).
- `EventEditorTarget` `new` takes an optional `recurrence` (quick add's Tab); `openNewEvent(..., fields,
  recurrence)`.
- Style slots: `gridStyle.ts`, `lookStyle.ts`, `viewsStyle.ts`; strings `calendar-grid.ftl`, `calendar-look.ftl`,
  `calendar-views.ftl`. The shared `Popover` (`[data-part="popover"]`) sets the UI font (portalled to the body).
- Icons: `repeat` (recurring cards) and `video` (call link chip) are bundled; adding one = `nail-api` `icons.rs`
  `BUNDLED_ICONS` + `ui/src/widgets/lucideIcons.ts` + regenerate `crates/nail-api/fixtures/icons.json`
  (`NAIL_UPDATE_SNAPSHOTS=1 cargo test -p nail-api`).

**Review fixes (after integration):**

- The cascade decision depends on the measured grid scale (`sideBySideMinutes(pxPerMinute, rootFontPx())`), so
  two events 15–29 minutes apart may stand side by side (narrower than 60 %); e2e checks "further right" and a
  whole visible title line instead of a fixed width.
- Use `DueTaskItem` for any due-task row (checkbox, drag with the raw title, Plan at `PimState.planSlot`),
  `writableTaskLists`, `workHourRange` / `workHoursValid` (vectors `calendar.json`), `peopleSummary`,
  `lengthText`, `useSchedulingRunner`, `useEventOverlaps` (core `ListEventOverlaps`) and the mock's
  `conflictsIn`; `ONE_LINE`, `FOCUS_RING`, `ICON_BUTTON` live in `theme/declarations.ts`.
- `SearchPim` hits of a series carry the next occurrence (the mock's `nearestStart` mirrors it).

**Pitfalls:**

- A `layout.kdl` change (the calendar window's 900 px breakpoints) needs `crates/nail-api/fixtures/
  default-layout.json` regenerated: `NAIL_UPDATE_SNAPSHOTS=1 cargo test -p nail-config --test integration
  defaults::`. Removing `theme.kdl` rules likewise regenerates `default-theme.json`.
- When a part is renamed, its rules in `defaults/theme.kdl`, `features/pim/skin.ts` / `parts.ts`, `skin.test.ts`
  and `e2e/visual/screens.ts` all go (the year view's month title is now `calendar.year.grid.title`).
- The full Playwright run is load-sensitive while other suites run on the machine: use few workers and rerun
  failures alone before suspecting a test.
- Store schema v25 is the colour backfill; a later migration takes v26.
