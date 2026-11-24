# Nail — calendar that reads well (R7, ADR 0246): texts of the calendar
# toolbar (search, quick add, time zones), narrow windows, the agenda,
# month, year and free-time views, empty states, the "Due this week"
# panel and the calendar preferences in Settings.
#
# Message ids start with `pim-views-`; shared verbs are common-*.

## Toolbar: quick add (S2)

pim-views-quick-add-label = Quick add an event
pim-views-quick-add-placeholder = Add an event, e.g. “Lunch with Bob tomorrow 12:30 at Corner café”
pim-views-quick-add-preview = What Nail understood
pim-views-quick-add-hint = Enter adds it · Tab opens the editor · Esc cancels
pim-views-quick-add-added = Added “{ $title }” on { $when }.
pim-views-quick-add-kind-location = Where

## Toolbar: search (S9)

pim-views-search-label = Search events
pim-views-search-results = Events found
pim-views-search-upcoming = Upcoming
pim-views-search-past = Past
pim-views-search-none = No events match “{ $query }”.

## Toolbar: time zones (C5)

pim-views-zone-chip-label =
    { $second ->
        [none] Time zone: { $zone }. Change the time zones
       *[other] Time zones: { $zone } and { $second }. Change the time zones
    }
pim-views-zone-chip-two = { $zone } + { $second }
pim-views-zone-title = Time zones
pim-views-zone-view = Show times in
pim-views-zone-system = This computer’s zone ({ $zone })
pim-views-zone-second = Second time zone
pim-views-zone-none = None
pim-views-zone-fixed = This calendar’s layout sets its own time zone; your choice applies to the other calendar views.

## Narrow windows (M7)

pim-views-drawer-open = Show calendars

## Mini-month and year (S5, C2)

pim-views-week-short = Wk
pim-views-week-number = Week number
pim-views-week-label = Week { $week }
pim-views-day-with-events = { $day }, has events

## Agenda (S7)

pim-views-tomorrow-with-date = Tomorrow, { $date }
pim-views-agenda-until = until { $time }
pim-views-agenda-now = Now · { $time }
pim-views-agenda-entry =
    { $title }, { $time }{ $place ->
        [none] {""}
       *[other] , { $place }
    }{ $past ->
        [yes] , over
       *[no] {""}
    }

## Free time (C1)

pim-views-free-total = { $length } free
pim-views-free-none = No free time
pim-views-free-busy = Busy: { $title }, { $time }
pim-views-free-busy-all-day = Busy all day: { $titles }

## Empty states (C4)

pim-views-empty-range = Nothing planned for these days.
pim-views-empty-today = Back to today
pim-views-empty-next = Next: { $title }, { $when }
pim-views-empty-previous = Last: { $title }, { $when }
pim-views-empty-today-agenda = Nothing planned today.
pim-views-tasks-empty = No open tasks. Add one with the field above or “New task”.

## Due this week (S12)

pim-views-due-title = Due this week
pim-views-task-plan = Plan “{ $title }” in the calendar
pim-views-due-empty = Nothing due this week.
pim-views-due-add = Add a task due today to { $list }

## Settings: calendar preferences (S6)

pim-views-prefs-title = Calendar preferences
pim-views-prefs-hours = Working hours
pim-views-prefs-start = Working day starts
pim-views-prefs-end = Working day ends
pim-views-prefs-midnight = Midnight
pim-views-prefs-hours-invalid = The working day must end after it starts.
pim-views-prefs-days = Work days
pim-views-prefs-week-start = Week starts on
pim-views-prefs-week-start-locale = As the language says ({ $day })
pim-views-prefs-week-numbers = Show week numbers
pim-views-prefs-show-declined = Show declined invitations (struck through)
pim-views-prefs-saved = Calendar preferences saved.
