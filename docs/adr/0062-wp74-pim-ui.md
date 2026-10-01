# ADR 0062: PIM UI — calendar views, editors, contacts, tasks, reminders (WP74)

Status: accepted · 2026-10-01 · M3a WP74

## Context

Plan section 8, WP74: calendar views (day, multi-day, week, work week,
month, year, agenda, free-time timeline) as layout widgets with drag to
move / resize and keyboard navigation, the mini-month and today's agenda
widgets, the event editor (recurrence incl. "this / this and following /
all", attendees, alarms, time zones, secondary zone, conferencing links),
contacts (list, card, editor, groups, merge dialog), the task list and
editor with subtasks, reminder toasts (snooze / dismiss); all themeable,
all strings in Fluent, axe-clean. The package builds against the contract
of ADR 0058 and owns only `ui/src/features/pim/` and
`locales/{en,de}/pim.ftl` (plus this ADR, a theme snippet the integrator merged into `defaults/theme.kdl`
and the user page `userDocs/en/calendar-contacts-tasks.md`); everything
else is requested from the integrator (end of this ADR).

## Decisions

### Date and time handling

- **`@internationalized/date` 3.12 (Apache-2.0)** does the calendar
  arithmetic: `PimTime` ↔ zoned / floating / date values (`time.ts`), zone
  conversion (IANA, DST), week starts **by the UI locale**
  (`startOfWeek(date, locale)`: a German UI starts on Monday in an English
  browser), view ranges and stepping. Chosen over hand-written `Date`
  arithmetic (wrong around DST, no locale week data) and over Temporal
  (not in every browser the remote UI supports yet).
- `i18n/format.ts` stays the only formatter: every date text goes through
  `useDateFormat()` with an explicit `timeZone` option (`fmt.ts`); calendar
  days are formatted as UTC noon so no zone moves them to another day. The
  source rule of `format.test.tsx` holds (no `Intl.DateTimeFormat` here).
- Positions in a day column are **wall-clock minutes** (`placeDay`): the
  grid always shows 24 hours, also on the days summer time starts or ends;
  overlapping events are laid out side by side in greedy columns per
  cluster. All-day and 24-hour-plus events go to the all-day row, packed in
  rows across the days they span (`placeSpanning`).

### The date / time picker

- Native `<input type="date|time|datetime-local">` follow the browser's or
  the system's locale, not Nail's UI language. The editors use **React
  Aria Components 1.21 (Apache-2.0; deps `react-aria`, `react-stately`,
  `@internationalized/*`, `@swc/helpers` Apache-2.0, `clsx` /
  `client-only` / `aria-hidden` MIT, `tslib` 0BSD)**: segmented
  `DateField` / `TimeField` (each part a spin button; typed or changed with
  the arrow keys; screen-reader names per segment) under React Aria's
  `I18nProvider` set to the UI locale, so the order of the parts, the 12 /
  24-hour clock and the segment names follow the UI language
  (`DateTimeInput.tsx`). A button opens our own month grid (`MonthGrid`,
  the same component as the mini-month) to pick the day by mouse or
  keyboard.
- React Aria sets a few **behavioural** inline styles (`unicode-bidi:
  isolate` on the field, `caret-color: transparent` on segments, a visually
  hidden native input for form autofill). They carry no look; every
  visible element has a `data-part` and is styled by the theme. Our own
  components never use `style` or class names (lint rules unchanged).
- The three editor dialogs load **on demand** (`lazyDialogs.tsx`,
  `editorDialogs.ts`): React Aria is ~200 kB of the build and is not
  needed until an editor opens. `loadPimEditors()` preloads them (tests).

### Data-driven geometry, structure and theme

- Event positions and sizes (top / height / left / width as percentages,
  `grid-column` / `grid-row` spans), the drag ghost, the "now" line, the
  free-time segments and **calendar colours** are data, so they go through
  the theme's geometry rules (`useGeometry`, `[data-geom]`), never inline
  styles. A calendar colour is `border-left-color` on the event (the theme
  decides the border width and style; swatches get `background-color`).
- The structure every theme needs (the time grid's layers: slots behind,
  events absolutely positioned over them; wrappers with `display:
  contents`; lists without bullets; the date popup and toasts) is
  `PIM_BASE_CSS` in `parts.ts`, to be appended to the compiled base CSS
  like `PLUGIN_BASE_CSS`. The default look (colours, borders, spacing,
  fonts, states) is in `defaults/theme.kdl` (merged by the integrator), checked with
  `nail_config::parse_theme_all` (only supported properties).
- States are `data-state` words (`selected`, `today`, `outside`, `busy`,
  `recurring`, `read-only`, `tentative`, `free`, `dragging`, `done`,
  `overdue`, `drop-target`, …), so themes write `part "x:today"`.

### Widgets, state and actions

- Widget kinds: `calendar`, `agenda`, `mini-month`, `task-list`,
  `contact-list`, `contact-card` (named by ADR 0058) plus **`calendar-list`**
  (calendars and task lists by source: show / hide, colour, rename, new
  local calendar or task list, buttons to the sources and import dialogs).
  Options are listed in `apiRequests` for `crates/nail-config/src/widgets/pim.rs`.
- One PIM store per UI store (`store.ts`, like the other features): the
  **day shown** is shared by every calendar, mini-month and agenda widget;
  `calendar.view` overrides the views of all calendar widgets; the selected
  contact / task, the editors' targets, the pending recurring change and
  the toasts live there.
- Actions: `calendar.goto-today`, `calendar.next`, `calendar.previous`,
  `calendar.view "<view>"`, `calendar.new-event`, `calendar.calendars`,
  `calendar.event-from-message`, `tasks.new`, `tasks.from-message`,
  `contacts.new`, `contacts.groups`, `contacts.merge`, `pim.import`,
  `pim.sources`, `pim.sync`. Key contexts: `calendar`, `contacts`, `tasks`
  (`data-key-context`).
- Dialogs (`window.open "<name>"`): `pim-event-editor`, `pim-edit-scope`,
  `pim-contact-editor`, `pim-contact-groups`, `pim-merge-contacts`,
  `pim-task-editor`, `pim-import`, `pim-sources`. Groups and merging are
  dialogs (fixed purpose), not widgets.

### Editing recurring events

- Editing one occurrence (from a view, a reminder) asks the scope in the
  editor (default **this occurrence**; the recurrence editor is locked for
  `this`). "This and following" sends the edited fields and recurrence with
  the occurrence's recurrence id (the core splits the series and returns
  `split`). "All occurrences" from an occurrence moves the series by the
  wall-clock amount the occurrence moved (`wallDelta`), keeps the series'
  zone unless the user chose another, and sends the series recurrence.
- Moving / resizing / deleting a recurring occurrence in a view (pointer
  or Alt + arrows / Delete) stores a `pendingChange` and opens
  `pim-edit-scope`; single events change at once. Moves are computed in
  the viewer's zone and written back in the event's own kind and zone
  (`movedTimes`; dates move by days; floating times stay floating).
- The recurrence editor covers daily / weekly (weekdays) / monthly (day or
  n-th / last weekday) / yearly, every n, ending never / after n / on a day
  (`UNTIL` in the start's kind; UTC for zoned starts, RFC 5545). Rules it
  cannot show (`BYSETPOS`, several rules, sub-daily, …) are kept as
  "custom" until replaced; exceptions and extra dates are always kept.

### Accessibility and keyboard

- Calendar grids are ARIA grids with one roving tab stop: month (arrows,
  Home / End, Enter shows the day), mini-month / picker / year (also Page
  Up / Down, Shift for years), the time grid's slot grid (arrows; Enter
  creates an event at that hour). Events are buttons in time order with a
  full name (title, time, day, calendar, "repeats"); Enter opens, Delete
  deletes, Alt + arrows move (time grid: 15 minutes / a day; day grids: a
  day / a week), Alt + Shift + Up / Down resize.
- Contacts are a multi-select list box (arrows, Shift + arrows, Space,
  Enter edits, Delete); tasks a tree (arrows, Left / Right collapse /
  expand, Space completes, Enter edits, Delete).
- `a11y.test.tsx` runs axe on every view (with a secondary zone), every
  widget, every dialog with real content, the open date picker and the
  toasts.

### Reminders

- `ReminderDue` and reminders already fired when the UI starts become
  in-app toasts (bottom left; plug-in notifications are bottom right) with
  Open, a snooze choice (5 / 10 / 15 / 30 / 60 minutes, tomorrow 09:00) and
  Dismiss / Dismiss all; failed PIM writes (`PimOpStatus` failed) too. OS /
  Web notifications stay with the core's `NotificationRequested` (desktop
  shell, triage feature).

### Mock core

- `mock.ts` is a working in-memory PIM (all 52 requests) on demo data
  relative to today (`mockSeed.ts`): edit scopes incl. the split, override
  shifting, EXDATE deletes, recurrence expansion keeping wall time across
  DST (`mockExpand.ts`; DAILY / WEEKLY / MONTHLY / YEARLY, BYDAY with
  ordinals, BYMONTHDAY, BYMONTH, COUNT / UNTIL, RDATE / EXDATE), free time
  in working hours, duplicates, merge (fold same book, link others), groups,
  subtasks and cycles, recurring completion, reminders, sources (written to
  `accountsConfig.pim_sources`), `.ics` / `.vcf` import of uploaded text,
  validation with the core's `pim-issue-*` ids. Writes answer op ids and
  report `PimOpStatus` later (`failNextOp` for tests). `unsupported: true`
  restores the contract-stage behaviour.

## Not done / deviations

- Drag on empty slots to create an event with a chosen length: creation is
  double-click / Enter on an hour (one hour) or the "New event" button.
- The contact card shows upcoming meetings (`SearchPim` by email) but not
  recent mail with the person (needs a mail search by address; follow-up).
- The free-time timeline's working hours are widget options
  (`work-start`, `work-end`; default 8–18); the core decides the free
  slots.
- Contact photos are limited to 512 KB images in the editor.
- No Playwright scenarios here (`e2e/` is integrator-owned; WP75 lists the
  M3a scenarios).

## Requests to the integrator

See the package report (`apiRequests`): widget options and the
`calendar-list` kind in `nail-config`, `PIM_BASE_CSS` in
`ui/src/theme/compile.ts`, the theme parts and icons, default layout
(calendar / contacts / tasks windows, actions, keys, toolbars, menus), the
e2e a11y sweep's dialog list, the feature README table, and the docs.
