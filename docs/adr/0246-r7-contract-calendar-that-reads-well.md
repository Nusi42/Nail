# 0246 — R7 contract: a calendar that reads well

Status: accepted (2026-11-24)

## Context

A design review of the calendar, tasks and invitations (week, day, month,
year, agenda and free-time views, the event editor, scheduling, the
invitation card, reminders, narrow windows) found 24 problems, ranked
must / should / could. The maintainer approved all of them:

- **Must:** every calendar is the same blue and events are a faint tint
  (M1); overlapping events become unreadable slivers (M2); 15- and
  30-minute events clip their own text (M3); one click on an event opens
  the full editor, which hides the week (M4); there is no drag to create,
  and a double-click creates a one-hour event on the full hour (M5); today
  looks like a shaded, disabled column and "now" is a thin rule over the
  event text (M6); narrow windows and the web mode on a phone clip the week
  (M7).
- **Should:** unanswered invitations look like any other event (S1); no
  quick create from text (S2); the editor is one long form (S3); event cards
  hide what the core already knows (repeats, people, reminders, read-only;
  S4); the mini-month is a plain number grid (S5); no working hours,
  weekends or week numbers (S6); the agenda is a stack of identical bars
  (S7); the month view repeats multi-day events and frames today heavily
  (S8); no search in the calendar (S9); the invitation card weighs every
  button the same and lists the iCalendar part as "Unnamed attachment"
  (S10); the scheduling grid is small and contradicts itself (S11); tasks
  never meet the calendar (S12).
- **Could:** the free-time view has no axis or labels (C1); the year view
  only bolds busy days (C2); the reminder toast lacks context (C3); empty
  states are one line (C4); time zones are hidden in layout options (C5).

Most of it is UI work on data the core already has. Five findings need the
core: distinct default colours (M1), the user's own participation status
(S1), reading a sentence as an event (S2), working hours and the other
calendar preferences (S6, C5) and tasks due in a range (S12). The wave runs
as a contract, four parallel packages (time grid, quick look and editor,
views and shell, core) and an integration.

## Decision

### Scope decisions

1. **Colours (M1).** The palette moves into the core:
   `nail_api::pim::calendar_view::CalendarColor` (nine colours, ids as
   before) with `CALENDAR_PALETTE` exported to TypeScript; the UI's
   `CALENDAR_COLORS` is that constant. A calendar created without a colour,
   or discovered without one (CalDAV `calendar-color`, Graph `hexColor`
   are honoured as they come), gets `next_calendar_color`: the first
   palette colour no other calendar uses, else the least used one (palette
   order breaks ties). Existing calendars without a colour get one the same
   way at their next sync (a local setting; written to the server only when
   the user changes it). Event cards show 20 % of the colour (was 16 %) with
   the solid leading edge; the contrast test keeps every palette colour
   readable in every built-in theme.
2. **Overlaps (M2):** cascade, each later overlapping event indented by
   about a fifth of the column and drawn on top with a hairline outline in
   the page colour, every title keeping at least 60 % of the column; events
   that do not overlap expand into free columns.
3. **Short events (M3):** below 45 minutes one line (title, then start
   time), a minimum height of about 18 px (`data-size="short"`).
4. **Quick look (M4):** a click (or Enter / Space on a focused card) opens
   a popover beside the event: time, calendar, place, people with their
   answers, the call link, and Edit / Delete / Join; RSVP for an
   invitation. Enter in it (or a double-click on the card) opens the full
   editor; Esc and a click outside close it.
5. **Drag to create (M5):** dragging on empty slots draws the event in
   15-minute steps with a live time label; on release an inline field takes
   the title (Enter saves, "More options" opens the editor with the times,
   Esc drops it). A click-and-release snaps to the 15-minute slot under the
   pointer. Keyboard: Shift + arrows on the slot grid extend the range.
6. **Today and now (M6):** today's date in a filled accent circle, no
   column shade; the now line 2 px with a dot on today, a faint line across
   the other days, the time in the gutter, under the event text.
7. **Narrow windows (M7):** below 900 px the calendar sidebar becomes a
   drawer; when a day column would be narrower than about 90 px the week
   falls back to three days, then one day; on narrow or touch layouts "New
   event" is a floating button. Breakpoints live in `layout.kdl` and the
   calendar's container queries.
8. **Invitations (S1, S10):** `EventInstance.my_status` is the user's
   `PARTSTAT` (an attendee with one of the user's addresses; `null` for the
   organizer or a non-attendee). `needs-action` looks provisional (dashed
   outline, no fill, a "?" mark); declined events are struck through, or
   hidden with the preference `show_declined` off. The invitation card gets
   one segmented answer control (Accept first), a mini day strip of what it
   overlaps, the attendee summary on one line and the secondary actions in
   a menu. The mail reader hides the `text/calendar` part from the
   attachment list when the invitation card shows it (UI only, no core
   flag).
9. **Quick create (S2):** the core reads text
   (`Request::ParseEventText`, crate `nail-event-text`): dates, times and
   ranges, lengths, "every …" repetitions, "all day" and "at …" (a place),
   returning the title, start / end, place, repetition and the recognised
   spans (UTF-16 offsets for the UI's marks). English only in R7; the
   keywords live in a per-language table in the crate (they are grammar,
   not UI texts); an unknown language reads as English. The calendar
   toolbar's field previews the result as chips; Enter adds the event to
   the default calendar, Tab opens the editor prefilled. The command palette
   and the MCP surface stay as they are in R7 (backlog).
10. **Editor (S3):** the title as a large field, one date / time row with
    the length ("Fri, Nov 27, 2026  3:00 PM → 4:00 PM  1 hour"), chips for
    repeat, reminder, calendar, busy / free and call link, people and notes
    below, time zone of the end, visibility, status and web page behind
    "More". Dates read as text in the UI language, not as numeric fields
    plus a separate button.
11. **Cards (S4):** small icons for `recurring`, `has_attendees`,
    `has_alarms` and `read_only`, hidden when the card is too narrow; free
    (`transparency`) events are hatched.
12. **Preferences (S6, C5):** `CalendarPreferences` in the app settings
    (`GetCalendarPreferences` / `SetCalendarPreferences`,
    `Event::CalendarPreferencesChanged`): working hours (default 8:00 –
    18:00), work days (default Monday – Friday), week start (default: the UI
    locale's), week numbers (ISO 8601, default off), the view zone (default:
    the system's), a second zone (default none), `show_declined` (default
    on). Settings → Calendar & contacts edits them; the toolbar's zone chip
    sets the view zone and the second zone. A calendar widget's layout
    options (`time-zone`, `secondary-zone`, `work-start`, `work-end`) still
    win for that widget. The grid shades time outside working hours and
    tints the weekend, opens scrolled to the start of the working day and
    shows the week number in its corner.
13. **Views (S5, S7, S8, C1, C2, C4):** the mini-month shows the range on
    screen as a band, event dots in calendar colours and week numbers; the
    agenda has a time column, a colour dot, faded past events, a now
    marker, "Tomorrow" like "Today" and "until 1:00 AM" on the second day
    of a midnight-crossing event; the month view draws multi-day events as
    one bar across their days, timed events as "dot, time, title" lines and
    today as a filled pill; the free-time view has an hour axis, labelled
    busy blocks, the reason for a busy day and the free total per day; the
    year view shades each day by booked hours; empty states offer "Back to
    today" and "New event" and name the nearest event outside the range.
14. **Search (S9):** a search field in the calendar toolbar (`/` focuses
    it, as in Mail) over `SearchPim` (events), results grouped upcoming /
    past; choosing one shows its day and opens its quick look.
15. **Scheduling (S11):** a wider grid with a lane per attendee, the chosen
    slot as a frame that can be dragged, suggestion chips from
    `FindFreeTime` / free-busy, busy details only where shared; no
    contradicting texts and no raw field labels.
16. **Tasks (S12):** `Request::ListTasksDue` lists open tasks due in a
    range. Due tasks show with a checkbox in the all-day row (timed ones on
    their day), the sidebar gets a "Due this week" panel, and dragging a
    task onto the grid creates an event with its title at that slot
    (`DEFAULT_EVENT_MINUTES` long); the task stays.
17. **Reminder toast (C3):** relative time ("in 30 minutes"), the
    calendar's colour on its edge, Dismiss as the primary action and snooze
    as one split button.
18. **Release:** the R7 items go into "Nail 0.8.11" of the release notes and
    `CHANGELOG.md` `[Unreleased]` (slots prepared by the contract);
    packages add their user-visible items, the integrator merges them.

### The core contract (this ADR's code)

- `nail_api::pim::calendar_view`: `CalendarColor` (+ `value`, `of_value`,
  `ALL`), `PaletteColor`, `calendar_palette()`, `next_calendar_color(used)`;
  `WorkHours` (minutes after midnight, end exclusive, at most
  `MINUTES_PER_DAY`), `CalendarPreferences` (+ `issues`: new
  `PimIssue::InvalidWorkHours`, `TooMany` for a repeated work day,
  `InvalidTimeZone`); `ParsedEventText` (+ `title_only`), `ParsedSpan`,
  `ParsedPart`; `MAX_EVENT_TEXT` (500 characters), `DEFAULT_EVENT_MINUTES`
  (60).
- `EventInstance.my_status: Option<ParticipationStatus>`; the contract
  fills `None` everywhere, the core package computes it (instance cache and
  `nail-ical` expansion; `nail-dav`'s free/busy already has the same idea
  as a closure).
- Requests (in `pim_event_task_requests!`, remote scope read; setting the
  preferences needs the calendar scope): `GetCalendarPreferences`,
  `SetCalendarPreferences { preferences }` → `Response::CalendarPreferences`;
  `ParseEventText { text, tz, now?, locale? }` →
  `Response::EventTextParsed { parsed }`; `ListTasksDue { from, to, tz,
  calendar_ids?, include_completed? }` → `Response::Tasks`.
- Implemented by the contract: the preferences (app-settings key
  `calendar-preferences`, checked, announced, restored from backups with
  their checks) and `ListTasksDue` (due dates and floating times placed in
  the request's zone, by due, the 400-day range limit of the instance
  list). Stubs for the core package: `nail-event-text::parse` (answers the
  whole text as the title) and `my_status` (`None`); the default colour on
  create / discovery is not wired yet.
- Generated TypeScript (`npm run gen:api`): the types, `CALENDAR_COLOR_VALUES`,
  `PARSED_PART_VALUES`, `CALENDAR_PALETTE`, `DEFAULT_CALENDAR_PREFERENCES`,
  `DEFAULT_EVENT_MINUTES`, `MAX_EVENT_TEXT`, `MINUTES_PER_DAY`.
- The UI mock answers all four requests (`features/pim/mockCalendarView.ts`:
  preferences with the core's checks, tasks due, a small reader for "today
  / tomorrow", one time or range, "all day" and "at …" (a place)) and fills
  `my_status` from the user's addresses (`userAddresses`, shared with the
  scheduling mock).

### Shared UI pieces (packages use them, never change them)

- `ui/src/widgets/Popover.tsx`: `Popover` (anchored, portalled to the body,
  focus on `[data-autofocus]` and back on close, Esc and a press outside
  close it; part `popover` or the caller's, `data-placement`),
  `placePopover` (pure), `anchorOf`, `usePopoverPosition` (geometry rule);
  `PimMenu` places its menu with `placePopover`. Base CSS
  `[data-part="popover"]` (fixed) and its look in the shared skin.
- `ui/src/features/pim/prefs.ts`: `useCalendarPreferences`,
  `saveCalendarPreferences`, `weekdayOf`, `isWorkDay`, `isWorkingTime`,
  `offHours`, `isoWeekNumber`, `weekStartOf`, `viewZonesOf` (layout option
  → preference → system), `useTasksDue`, `TASK_DRAG_TYPE` /
  `setTaskDrag` / `taskOfDrag`, `parseEventText`.
- `PimState.quickLook` / `setQuickLook` (`QuickLook`: the instance and its
  card's rectangle) and `EventPopover.tsx`'s `EventPopoverHost`, mounted
  once through the feature's `effects` (the quick-look package fills it).
- `colorOf` (`features/pim/common.tsx`) stays the one rule for an event's
  colour (its own, else its calendar's).
- Style slots, one per UI package, appended to `PIM_BASE_CSS` /
  `PIM_SKIN_CSS`: `features/pim/gridStyle.ts`, `lookStyle.ts`,
  `viewsStyle.ts`. The R7 parts get their look there rather than in
  `defaults/theme.kdl`, so the default-theme fixture does not change in
  parallel; a package that removes a `theme.kdl` rule (the today column
  shade) reports it and the integrator regenerates the fixture.
- Locale slots `locales/{en,de}/calendar-grid.ftl`, `calendar-look.ftl`,
  `calendar-views.ftl` (message ids `pim-grid-*`, `pim-look-*`,
  `pim-views-*`); `pim.ftl` and `common.ftl` only through the integrator.

## Consequences

- The four packages build against fixed shapes and do not touch each
  other's files: the time grid package (`TimeGrid.tsx`, `EventItem.tsx`,
  `time.ts` `placeDay`, `newEvent.ts`, `dayDrag.ts` and their tests,
  `gridStyle.ts`, `calendar-grid.ftl`), the quick look and editor package
  (`EventPopover.tsx`, `EventEditor.tsx`, `RecurrenceEditor.tsx`,
  `Reminders.tsx`, `DateTimeInput.tsx`, `features/scheduling/`, the mail
  reader's `text/calendar` part, `lookStyle.ts`, `calendar-look.ftl`), the
  views and shell package (`Calendar.tsx`, `CalendarViews.tsx`,
  `MonthGrid.tsx`, `SmallWidgets.tsx`, `CalendarList.tsx`,
  `CalendarDialogs.tsx`, `colors.ts`, `TaskList.tsx`, `PimSettings.tsx`, the
  layout breakpoints, `viewsStyle.ts`, `calendar-views.ftl`) and the core
  package (the crates).
- Until the core package lands, the real core reads every text as a title
  and reports no `my_status`; the mock already answers both, so UI tests and
  mock mode work.
- Colours are core data: a client that creates a calendar without a colour
  gets the same default as the UI would show.
- The quick look changes what a click on an event does: tests and docs
  that expect the editor after one click move to Enter or a double-click.
