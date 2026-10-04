# ADR 0113: M5b WP164 — Calendar view redesign, calendar list menus, event cards, PIM settings section

Status: accepted · 2026-10-03 · M5b WP164 (the integrator renumbers)

## Context

Plan section 10, wave M5b, WP164: redesign the calendar view as in the approved
proposal (a published page, not linked here, "Calendar and
Settings get their own space"): one toolbar line with Today, previous / next and
the range that never wraps, a segmented view switch, a side panel with the
mini-month and a calendar list with colour squares and per-calendar "…" menus
instead of buttons, "New calendar" in a menu instead of the inline form, and
readable event cards; the same polish for Contacts and Tasks; and the
calendar / contacts section of WP163's settings page (contract ADR 0109).

WP164 owns `ui/src/features/pim/` and `locales/{en,de}/pim.ftl` only. The
layout (`defaults/layout.kdl`), the widget specs (`nail-config`), the shipped
part rules (`defaults/theme.kdl`), e2e and user docs belong to others; what they
need is listed as requests for WP165 (below).

## Decisions

### Toolbar: one line, shared primitives

The `calendar` widget's header is now `calendar.toolbar` (a new part, so the
old `calendar.header` rule with `flex-wrap: wrap` in `defaults/theme.kdl` no
longer applies). Order: Today (`form.button`, `data-field="calendar-today"`),
previous / next (`form.button` `data-variant="icon ghost"`, names "Previous" /
"Next"), the range (`calendar.title`, one line, ellipsis, its full text as
`title`), the view switch, "New event" (`form.button` `data-variant="primary"`).
The skin keeps the toolbar `flex-wrap: nowrap`; the title is the only item that
shrinks; the switch scrolls horizontally rather than wrapping.

The view switch is the shared `segmented` look (`data-field="calendar-view"`)
but keeps **toggle buttons with `aria-pressed`** inside a labelled group
instead of the `Segmented` primitive's radios, so the existing e2e scenarios
(`getByRole("button", { name: "Agenda" })`) and the `calendar.view` action keep
working. The contract's `view-switch` option is implemented: `segmented`
(default) or `menu` (a labelled drop-down for narrow panes). Which views appear
stays the layout's `views` option (the default layout still lists `work-week`;
the plan's Day / Week / Month / Year / Agenda / Free time is a layout change for
WP165).

### Event cards: tint + bar, text always `fg`

Every occurrence (time grid `calendar.event`, all-day / month `calendar.chip`,
agenda rows) renders title, time and location as `event-card.title` /
`.time` / `.location` (title first; chips show the time first via CSS `order`,
the DOM order and the accessible name are unchanged). The calendar colour
reaches the card only through a geometry rule: `border-left-color: <colour>`
and `background-color: color-mix(in srgb, <colour> 16%, var(--nail-color-bg))`
(`colors.ts`, `EVENT_TINT`). The card text is always the theme's `fg` — never
`fg-muted` — so contrast does not depend on the calendar colour:
`colors.test.ts` checks `fg` against the tinted background for the palette, the
demo colours and extremes (black, white, pure primaries) in Frost, Sage,
Graphite, Nocturne and the base theme's light / dark modes (≥ 4.5:1). High
contrast drops the tint (`:root[data-nail-mode="high-contrast"]` rule, which
outranks the geometry rule) and outlines the card in `fg` while keeping the
calendar-coloured leading bar (≥ 7:1 for the text). Only `#rgb` / `#rrggbb` /
`#rrggbbaa` and colour keywords are accepted from data (`isCssColor`).

### Calendar list: groups, colour squares, menus, dialogs

`calendarGroups.ts` (pure): one group per source — this computer first, then
each CalDAV / Exchange source — and every ICS subscription together in
"Subscriptions" at the end; `show-sources=false` gives one group. New calendars
are offered only where the core accepts them (local, CalDAV). Removal:
subscriptions are *unsubscribed* (`RemovePimSource`, the core refuses deleting a
subscription's calendar), writable local / CalDAV calendars are deleted
(`DeleteCalendar`), anything else has no remove entry.

Rows: the visibility checkbox is drawn as the colour square
(`calendar-list.toggle`, filled when shown, hollow when hidden; the hollow rule
outranks the colour's geometry rule), then the name, a "Tasks" tag for task
lists and the "…" menu (`calendar-list.menu`, visible on hover / focus / open,
always on touch): Settings…, Hide / Show in the calendar, the colour palette as
`menuitemradio`s under a "Colour" header, Remove… / Unsubscribe…. Each group
header has a "+" menu (`calendar-list.add`): New calendar, New task list (where
allowed), New subscription…, Calendars and address books…, Import…. The
`actions=buttons` option shows settings / remove as icon buttons instead.

The menus are a small local menu-button component (`PimMenu.tsx`, WAI-ARIA
menu button: Enter / Space / Down open on the first item, Up on the last,
arrows / Home / End move, Escape returns focus, Tab and outside clicks close)
that reuses the shared `context-menu.*` parts so it looks like every other menu;
the layout-action `ContextMenu` cannot run local commands.

The inline "new calendar" form and the inline settings form are gone. Three
dialogs (`CalendarDialogs.tsx`) replace them: `pim-calendar-settings` (name,
colour, reminders; read-only calendars change only locally, which the core
allows), `pim-new-calendar` (in the source of the group whose "+" opened it,
kind preset by the menu entry, an unused palette colour preselected) and
`pim-remove-calendar` (asks before deleting / unsubscribing). Colours: a
nine-colour palette (`CALENDAR_COLORS`) as radio swatches plus "Other colour"
for any hex value. Their targets live in the PIM store (`calendarSettingsId`,
`newCalendar`, `removeCalendarId`).

### Mini-month, contacts, tasks

Mini-month navigation uses icon `form.button`s (the old `‹ ›` text spans are
gone there). Contact avatars get `data-color` from the shared avatar palette
(`colorSlot`, same as mail rows); list rows, cards and task rows (pills for due
/ priority / subtasks, overdue in `danger`) are restyled in the skin.

### The skin

`ui/src/features/pim/skin.ts` (`PIM_SKIN_CSS`) holds the look on tokens only
(literal colours only as token fallbacks; `skin.test.ts`). The compiler already
emits it after the shared skin and before the theme's part rules. Because the
part rules of `defaults/theme.kdl` (M3a) come later, they still override the
skin for every part they style; `PIM_SKIN_PARTS` lists the parts the skin
styles so the integration can remove the overlapping legacy rules (request
below). The redesigned elements that matter most (toolbar, segmented switch,
event-card text, calendar-list squares and menus, the dialogs) use new parts or
shared primitives and already show without that clean-up; the event colours
come from geometry rules, which win over theme rules anyway.

### Settings page section

A new widget `pim-settings` (the "Calendar & contacts" section): the calendar
list (buttons mode, not a landmark), the sources panel (the sources dialog's
body without Close, `SourcesPanel`) and Import. `Feature.settings` registers its
labels (`PIM_SETTINGS_INFO`, keywords `pim-settings-keywords`) and the
`calendar-list` labels for the settings search. The section label is
`@pim.settings.section` ("Calendar & contacts", in `pim.ftl`). The layout and
the widget spec are WP165's (requests below).

## Requests for the integration (WP165)

- `crates/nail-config/src/widgets/pim.rs`: register `widget("pim-settings", &[], &[], false)`.
- `defaults/layout.kdl` `settings-page`, group `general`, after `mail`:
  `section "calendar" label="@pim.settings.section" icon="calendar" { pim-settings id="page-pim-settings" }`;
  mirror into `crates/nail-api/fixtures/default-layout.json`.
- Calendar view window per the proposal: `calendar id="calendar-main" view="week"
  views="day week month year agenda free-time"` (drop `work-week` from the
  switch), the sidebar `mini-month` + `calendar-list` (+ optionally the task
  list).
- `defaults/theme.kdl`: remove the M3a rules (all states / conditions /
  ancestor variants) of the parts in `PIM_SKIN_PARTS` that it styles, and the
  now-dead `calendar.header`, `calendar.nav.previous|next|today|icon`,
  `calendar.views`, `calendar.views.button`, `calendar.new(.icon)`,
  `calendar.event.time|title|location|badge`, `calendar.chip.time|location|badge`,
  `calendar.agenda.event.badge`, `agenda.list.event.badge`,
  `mini-month.previous|next|icon`; keep `nail-config/tests/theme_contrast.rs`
  green. Then regenerate the visual baselines (calendar screen).
- e2e: scenarios for the calendar list menus (recolour, hide, new calendar via
  "+", unsubscribe), `view-switch`, and the settings section.

## Consequences

- Calendar parts renamed: `calendar.header` → `calendar.toolbar`; nav / view /
  new buttons are `form.button` / `segmented.option` with `data-field`; event
  text parts are `event-card.*`. User themes that styled the old names need the
  new ones (`userDocs` theme reference).
- `pim-calendar-new`, `pim-calendar-holds*`, `pim-calendar-settings` were removed
  from `pim.ftl`; new keys are under "M5b, WP164".
