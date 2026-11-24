# 0249 — R7 calendar views and shell

Status: accepted (2026-11-24)

## Context

ADR 0246 sets the scope of "a calendar that reads well". The views and
shell part (M1 fill, M7, S2, S5, S6 settings, S7, S8, S9, S12 sidebar, C1,
C2, C4, C5 toolbar) needed a few choices the contract left open: how a
narrow window gets a drawer without a new layout container, where the
"Due this week" panel lives, how the toolbar takes three more controls, and
how the month and year views get their new look without fighting the
default theme's older part rules.

## Decision

1. **Drawer from the layout's breakpoint.** `defaults/layout.kdl` gives the
   `calendar` window two breakpoints: from 900 px the sidebar split as
   before, below it the toolbar and the calendar alone. The `calendar`
   widget looks at the layout it is rendered in (`layoutShowsSidebar`: the
   breakpoint of its window for the current width) and, when that has no
   `mini-month` or `calendar-list`, offers **Show calendars**: a drawer with
   the mini-month, the calendar list and "Due this week". No new layout
   container and no new widget option; a user layout that keeps a sidebar
   at any width never sees the drawer.
2. **Fewer days instead of narrow columns.** The widget measures its width;
   a time grid whose columns would be narrower than `MIN_DAY_COLUMN_PX`
   (90) shows three days (`multi-day`), then one (`day`). The view switch
   keeps the chosen view; `PimState.shown` holds the view on screen, and
   `calendar.next` / `.previous` page by it.
3. **Floating New event** on narrow layouts (drawer mode) and touch screens
   (`pointer: coarse`), instead of the toolbar's button, so there is only
   one "New event".
4. **Toolbar:** the zone chip sits on the toolbar line before New event;
   quick add and the search share a second row under it
   (`calendar.tools`), with the quick-add chips below its field, so the
   toolbar line keeps room for the segmented view switch. `/` reaches the search through the
   existing `search.focus` request, so no new action or binding is needed.
5. **Due this week** ends the `calendar-list` widget (the sidebar of the
   default layout and the drawer), not a new widget kind.
6. **Own parts for the new looks.** Where the default theme still styles an
   M3a part the new look replaces, the views use new part names instead of
   overriding the theme: the year view's grid is `calendar.year.grid`, the
   agenda uses `….day-head`, `….rows`, `….entry`, `….time`, `….dot`,
   `….title`, `….place`, `….now`, the free-time view's labelled busy blocks
   `calendar.free-time.block`, the month view's `calendar.month.bar` /
   `.line` / `.lanes`. All look rules live in `viewsStyle.ts`.
7. **Agenda entries open the quick look** on a click (and Enter / Space),
   the editor on a double-click, like the time grid's cards; month cards
   keep the shared `EventItem`.
8. **Quick create's default calendar** is the editor's rule for a new event
   (a writable event calendar of this device, else the first writable one;
   `defaultEventCalendar`); without a parsed time the event starts at 9:00
   on the shown day (`DEFAULT_NEW_EVENT_TIME`, also New event's time).
9. **Year heat:** booked minutes per day (busy timed events by their
   minutes, busy all-day events as a working day) in five levels
   (`HEAT_STEPS_HOURS`: more than 0, 2, 4, 6 hours).

## Consequences

- The default layout fixture (`crates/nail-api/fixtures/default-layout.json`)
  changes with the calendar window's breakpoints.
- The default theme's rules for `calendar.year.month*`, `calendar.agenda.date`,
  `calendar.agenda.list`, `agenda.list.date`, `agenda.list.list`,
  `agenda.list.empty` and `calendar.free-time.busy` no longer match
  anything and can go (with the theme fixture); so can the skin's rules for
  `calendar.agenda.event` / `agenda.list.event`, the month cell's today
  frame and `calendar.year.month.title`.
- End-to-end specs that clicked an agenda entry to edit it double-click it.
