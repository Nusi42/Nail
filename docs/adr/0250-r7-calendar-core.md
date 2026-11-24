# 0250 — R7 core: reading text, the user's answer, default colours, working hours

Status: accepted (2026-11-24)

## Context

ADR 0246 fixed the shapes of R7 and left the core's behaviour to this
package: the reader behind `ParseEventText`, `EventInstance.my_status`,
default calendar colours on create and discovery, and one source of truth
for the working day. This ADR records how they work and the choices made
where the brief left room.

## Decision

### Reading a sentence (`nail-event-text`)

- The text is cut into words (whitespace and `, ; ! ? ( ) [ ] "` end a
  word, trailing dots are dropped, a leading `@` is a word of its own). At
  each word the grammar tries, in this order: a repetition, "all day", a
  date, a length, a time. Of each kind only the first phrase counts; a
  second one stays in the title. The place is what follows the first unread
  "at" / "@" up to the next read part. The title is the text without the
  read parts (words that are only punctuation dropped).
- Keywords live in `keywords.rs`, one table per language (ADR 0246: they
  are grammar, not UI texts). English is the only table; any other
  language reads as English. Numeric dates follow the region: `12/4` is
  4 December in `en` / `en-US`, 12 April in `en-GB`, `en-AU`, `en-NZ`,
  `en-IE`, `en-IN` and `en-ZA`.
- Ambiguities and how they are read (all pinned by the table tests):
  - A plain weekday is the coming one, today included ("Wednesday" on a
    Wednesday is today); "next Friday" is a week after that; "this Friday"
    is the plain reading.
  - "sat", "sun" and "wed" are words unless written with a capital letter
    or after "on" / "next" / "this" / "every". Month names count only next
    to a day number, so "May" and "March" stay words.
  - A month and day without a year is the next one (today included);
    29 February the next leap year.
  - A bare number is a time only after "at" / "from"; then 1 to 6 mean the
    afternoon ("at 3" = 15:00). Times with minutes are 24-hour.
  - In a range the half written on one end applies to the other
    ("3-4pm"); a start that would come after the end takes the other half
    ("11-1pm" = 11:00 to 13:00); an end not after the start is the next
    day ("11pm-1am").
  - A time without a date is today, or tomorrow once it has passed.
  - "all day" wins over a time; a range wins over a length; "for 3 days"
    makes an all-day event of three days.
  - A length alone ("Focus for 90 min") starts at the next quarter hour.
  - A repetition without a date starts on its first day (the coming
    named weekday, today for "daily"); without a time it is all-day.
  - A bare "m" or "h" after a separate number is not a length ("Run 5 m").
- "Now" is the request's instant placed in the request's zone by the core,
  so "tomorrow" late in the evening UTC is already the day after in
  Berlin. Times are zoned in the request's zone.

### The user's answer (`my_status`)

- One rule, `EventFields::my_status(is_mine)`: the `PARTSTAT` of the first
  attendee with one of the user's addresses; `None` when the user is the
  organizer or not an attendee. The brief's "organizer = accepted" is read
  as the contract states it (`None`): both draw as a committed event, and
  `None` keeps "the user is no attendee" distinct for the UI.
- The user's addresses are `Service::me` (every identity's address — the
  aliases — and every account's own address), compared like iTIP does
  (`nail_itip::address`).
- The status is computed when instances are read
  (`Store::list_event_instances_for`), from the occurrence's own fields (an
  override's attendees). The instance cache needs no column and no
  migration, and answers follow identity changes at once.
  `nail-ical`'s expansion leaves `my_status` `None`: its instances only
  feed the cache, reminders and conflict checks, never an API answer.
- The user's own free/busy (`nail_dav::own_busy_periods`) reads
  `instance.my_status` instead of a closure of its own.

### Default colours

- `Store::save_calendar` is the one place: a calendar row that is new and
  has no colour gets `next_calendar_color` over the colours stored on every
  other calendar. Create (`CreateCalendar`, the local defaults, imports),
  CalDAV / ICS discovery and Exchange discovery all go through it, so a
  colour from the server or the user always wins and the default is
  deterministic for the same calendars. Saving an existing calendar
  without a colour keeps its stored colour (two sync passes may both have
  read the calendar before either stored it); only an explicit settings
  update (`UpdateCalendar`) clears one. Colours are distinct across all of
  the user's calendars, not per account, because they share one list.
- Server colours: CalDAV `calendar-color` (Apple namespace) as it comes
  (also `#rrggbbaa`); Graph `hexColor` arrives as
  `ExchangePimFolder::hex_color` and wins over the preset `color` (an
  empty `hexColor` means the user never chose one); EWS has no calendar
  colour. Google calendars come through CalDAV and its `calendar-color`.
- A default colour on discovery stays local; it reaches a CalDAV server
  only when the user later changes the calendar (the server-change rule of
  `UpdateCalendar`). A new calendar created in Nail is created with it.
- Existing calendars: schema v25 has no table change and a backfill that
  runs once. Every calendar whose stored settings have no colour (before
  R7 that meant the theme's one default blue; a colour the user picked is
  always stored) gets the next palette colour in list order (order
  setting, name, id), counting the colours already given. Unknown fields
  of the stored settings are kept. Later, a calendar the user saves
  without a colour keeps none: only new rows are coloured.

### The working day

- `CalendarPreferences.work_hours` / `work_days` are the source of truth.
  The core had no other working-hours code; the MCP tool `find_free_time`
  gets `working_hours` (opt-in, so existing clients keep their answers):
  `day_start` / `day_end` default to the preferences' hours and slots on
  other days are dropped. A calendar widget's layout options stay
  per-widget overrides in the UI (ADR 0246).

### Remote and MCP

- The four requests already have their remote scopes (ADR 0246). MCP:
  `list_tasks` gets `due_after` / `tz` (answered by `ListTasksDue`, by due,
  the range ending at `due_before` or 30 days later); `ParseEventText`
  stays off MCP (backlog, ADR 0246). The preferences are read by
  `find_free_time` only.
- Invitations: the reader hides the `text/calendar` part in the UI (ADR
  0246); no core flag was needed.

## Consequences

- Quick create works in the real core; the UI's mock reader stays smaller
  (only the contract's subset) and the core's table tests are the
  reference.
- Upgrading to schema v25 changes the stored colour of calendars that had
  none; a downgrade keeps the colours (older builds read them as chosen).
- `ExchangePimFolder` has a new optional field (serde default), so stored
  or recorded folder lists without it still read.
