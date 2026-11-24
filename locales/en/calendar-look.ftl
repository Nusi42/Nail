# Nail — calendar that reads well (R7, ADR 0246): texts of the quick look
# next to an event, the compact event editor, dates and lengths as text,
# the reminder toast, the invitation card and the scheduling grid.
#
# Message ids start with `pim-look-`; shared verbs are common-*.

## Lengths ("1 hour", "1 hour, 30 minutes"; the UI joins the units as a list)

pim-look-length-minutes =
    { $count ->
        [one] 1 minute
       *[other] { $count } minutes
    }
pim-look-length-hours =
    { $count ->
        [one] 1 hour
       *[other] { $count } hours
    }
pim-look-length-days =
    { $count ->
        [one] 1 day
       *[other] { $count } days
    }

## The quick look next to an event

pim-look-loading = Loading the details…
pim-look-when = { $range } · { $length }
pim-look-when-zone = { $range } · { $length } · { $zone }
pim-look-calendar = { common-calendar }
pim-look-place = { common-where }
pim-look-people = People
pim-look-people-count =
    { $count ->
        [one] 1 person
       *[other] { $count } people
    }
pim-look-people-summary = { $people } · { $answers }
pim-look-answers-accepted = { $count } accepted
pim-look-answers-tentative = { $count } maybe
pim-look-answers-declined = { $count } declined
pim-look-answers-pending = { $count } not answered
pim-look-person =
    { $role ->
        [organizer-you] { $name } (organizer, you)
        [organizer] { $name } (organizer)
        [you] { $name } (you)
       *[other] { $name }
    }
pim-look-person-you = you
# An answer inside a sentence ("Bob maybe", "you not answered").
pim-look-status-needs-action = not answered
pim-look-status-accepted = { itip-status-accepted }
pim-look-status-declined = { itip-status-declined }
pim-look-status-tentative = maybe
pim-look-status-delegated = { itip-status-delegated }
pim-look-more-people = { agents-more-targets }
pim-look-join = { itip-label-join }
pim-look-send-update = Send update
pim-look-update-sent = The update is on its way.
pim-look-read-only = Read-only
pim-look-overlaps =
    { $count ->
        [one] Overlaps { $titles }
       *[other] Overlaps { $count } events: { $titles }
    }
pim-look-keys = Keys
pim-look-key-enter = { context-menu-key-enter }
pim-look-key-delete = { context-menu-key-delete }
pim-look-key-escape = { context-menu-key-escape }
pim-look-key-edit = edit
pim-look-key-delete-action = delete
pim-look-key-close = close

## The compact event editor

pim-look-place-placeholder = Add a place or room
pim-look-notes = { common-notes }
pim-look-arrow = to
pim-look-length = Length: { $length }
pim-look-options = { print-options }
pim-look-reminder = Reminder
pim-look-reminder-none = No reminder
pim-look-reminder-at-start = At the start
pim-look-reminder-before = { $length } before
pim-look-reminder-custom = Custom reminders
pim-look-call-link = Call link
pim-look-people-none = No one invited yet.
pim-look-end-date = End date
pim-look-date-choose = { $field }: { $date }

## The reminder toast

pim-look-reminder-meta = { $time } · { $calendar }
pim-look-snooze = Snooze { $length }
pim-look-snooze-more = More snooze times

## The invitation card

pim-look-invitation-from = from { $name }
pim-look-attendee =
    { $organizer ->
        [yes] { $name } (organizer) { $status }
       *[other] { $name } { $status }
    }
pim-look-strip-label = { $title } and what it overlaps
pim-look-strip-all-day = Also all day: { $titles }
pim-look-add-note = Add a note

## The scheduling grid

pim-look-fb-role = Role of { $name }
pim-look-fb-source = { $email } · { $source }
pim-look-fb-free-chosen = Everyone with shared availability is free at the chosen time.
pim-look-fb-other-times = Other times when everyone is free:
pim-look-fb-free-times = Times when everyone is free:
pim-look-fb-none-day = There is no time on this day when everyone is free.
pim-look-fb-try-next = Try the next day
pim-look-fb-unknown =
    { $count ->
        [one] 1 person shares no availability and is not counted.
       *[other] { $count } people share no availability and are not counted.
    }
pim-look-fb-slot = Meeting time: { $time }
pim-look-fb-hint = Drag the frame or use the arrow keys on it to move the meeting, or pick a suggestion.
pim-look-fb-private = No details shared
