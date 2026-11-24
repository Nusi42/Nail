# Nail — calendar that reads well (R7, ADR 0246): texts of the time grid
# (event card icons, the now line, drawing a new event and its inline
# title, working hours and week numbers, tasks in the all-day row).
#
# Message ids start with `pim-grid-`; shared verbs are common-*.

## Event cards

# An event card's accessible name: the card's label, then the user's answer to an invitation.
pim-grid-event-label = { $label }{ $answer ->
        [needs-action] , not answered yet
        [declined] , declined
       *[other] { "" }
    }

## Week numbers, tasks

pim-grid-week-number = { pim-views-week-short } { $week }
pim-grid-tasks-due = Tasks due

## Drawing a new event

pim-grid-new-event-when = { $day } · { $time }
# The drawn range while dragging: its times and its length.
pim-grid-draft-time = { $time } · { $length }
