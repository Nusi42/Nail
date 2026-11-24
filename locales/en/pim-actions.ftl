# Nail — backlog features of the calendar and PIM (R8, ADR 0251): right-click
# menus of events, contacts and tasks, the palette's "New event from text",
# the event editor's conflict choice, recent mail on the contact card and
# answering one occurrence from the quick look.
#
# Message ids start with `pim-act-`; shared verbs are common-*.


## "New event from text" (the command palette)

pim-act-from-text-title = New event from text
pim-act-from-text-keywords = quick add create sentence type calendar meeting appointment
window-pim-event-from-text = { pim-act-from-text-title }
pim-act-from-text-intro = Type the event as a sentence, for example “Lunch with Bob tomorrow 12:30 at Corner café”. Enter adds it, Tab opens the editor.
pim-act-from-text-no-calendar = There is no calendar to add events to. Add a calendar first.

## Recent mail on the contact card

pim-act-recent-mail = Recent mail
# $subject: the message's subject; $when: its date and time.
pim-act-recent-mail-open = Open “{ $subject }”, { $when }

## Undo for invitation answers

pim-act-reply-waiting = Your answer goes out in a moment.
pim-act-reply-undone = Your answer was not sent. Your calendar keeps it.
