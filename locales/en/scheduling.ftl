# Nail — English UI strings: invitations and scheduling (the invitation
# card in the reader, answering with a comment, proposing a new time,
# organizer updates / cancellations, the attendee picker with the
# free/busy timeline, scheduling settings; M3b WP84; contract ADR 0065).
#
# Owned by WP84; message ids start with the prefixes listed in
# ui/src/features/scheduling/messages.ts. Texts the core builds
# (invitation and free/busy problems) are in core-scheduling.ftl.

## Dialog titles (window-<dialog name>)

window-scheduling-cancel-meeting = Cancel meeting
window-scheduling-settings = { scheduling-action-settings }
window-scheduling-find-time = { scheduling-action-find-time }

## Actions (layout labels @scheduling.action.*)

scheduling-action-accept = Accept invitation
scheduling-action-tentative = Answer maybe
scheduling-action-decline = Decline invitation
scheduling-action-find-time = Find a time
scheduling-action-settings = Invitation settings

## Common

scheduling-minutes =
    { $minutes ->
        [one] { $minutes } minute
       *[other] { $minutes } minutes
    }
scheduling-organizer-unknown = the organizer
scheduling-repeats-custom = Repeats on a custom schedule

## Dates and times

scheduling-time-same-day = { $day }, { $start } – { $end }
scheduling-time-all-day = { $day } (all day)

## The invitation card

invitation-label = { itip-subject-request }
invitation-kind-publish = { common-event }
invitation-kind-request =
    { $state ->
        [update] Updated invitation
        [outdated] Older version of an invitation
       *[other] Invitation
    }
invitation-kind-reply = Answer to your invitation
invitation-kind-add = More occurrences added
invitation-kind-cancel = { common-cancelled }
invitation-kind-refresh = Request for the latest version
invitation-kind-counter = New time proposed
invitation-kind-decline-counter = Your proposal was declined
invitation-state-new = { common-not-answered-yet }
invitation-state-current = Your calendar is up to date
invitation-state-update =
    { $method ->
        [reply] Not recorded in your calendar yet
       *[other] Changes your calendar
    }
invitation-state-outdated = A newer version exists
invitation-state-cancelled =
    { $method ->
        [cancel] The organizer cancelled this event
       *[other] Cancelled
    }
invitation-state-unknown-event = This event is not in your calendar
invitation-state-invalid = This invitation cannot be used
invitation-issues = Problems with this invitation
invitation-my-status = Your answer
invitation-call = Call
invitation-reply-from =
    { $name } { $status ->
        [accepted] accepted.
        [declined] declined.
        [tentative] might attend.
        [delegated] delegated the invitation.
       *[other] has not answered yet.
    }
invitation-counter-from = { $name } proposes another time:
invitation-counter-location = Proposed place: { $location }
invitation-comment-from = Note from { $name }
invitation-attendees-summary =
    { $count ->
        [one] 1 person
       *[other] { $count } people
    }: { $accepted } accepted, { $tentative } maybe, { $declined } declined, { $pending } not answered
invitation-attendee-room = Room
invitation-attendee-resource = Resource
invitation-conflicts-none = You have nothing else at this time.
invitation-conflicts-title =
    { $count ->
        [one] This overlaps one event in your calendar:
       *[other] This overlaps { $count } events in your calendar:
    }
invitation-conflict-all-day = all day
invitation-conflict-tentative = (maybe)
invitation-transport-server = Your calendar server sends your answer to the organizer.
invitation-action-accept = { mcp-tool-value-accept }
invitation-action-tentative = { common-maybe }
invitation-action-decline = { mcp-tool-value-decline }
invitation-action-propose-new-time = Propose a new time
invitation-action-add-to-calendar = Add to calendar
invitation-action-update-calendar = Update my calendar
invitation-action-remove-from-calendar = Remove from calendar
invitation-action-apply-reply = Record the answer
invitation-action-accept-counter = Accept the new time
invitation-action-decline-counter = Decline the new time
invitation-action-send-current = Send the latest version
invitation-action-refresh = Ask for the latest version
invitation-action-forward = { scheduling-forward-title }
invitation-action-open-event = Open in calendar
invitation-done-accept = Accepted. No answer was sent.
invitation-done-accept-sent = Accepted. Your answer is on its way.
invitation-done-tentative = Marked as maybe. No answer was sent.
invitation-done-tentative-sent = Marked as maybe. Your answer is on its way.
invitation-done-decline = Declined. No answer was sent.
invitation-done-decline-sent = Declined. Your answer is on its way.
invitation-applied-add-to-calendar = Added to your calendar.
invitation-applied-update-calendar = Your calendar is updated.
invitation-applied-remove-from-calendar = Removed from your calendar.
invitation-applied-apply-reply = The answer is recorded.
invitation-applied-send-current = The latest version is on its way.
invitation-applied-accept-counter = The meeting moved; everyone gets the update.
invitation-applied-decline-counter = The proposal was declined.
invitation-applied-refresh = The organizer was asked for the latest version.
invitation-applied-forward = The invitation was forwarded.
invitation-applied-propose = Your proposal is on its way.

## Answering

scheduling-answer-accept = { mcp-tool-value-accept }
scheduling-answer-tentative = { common-maybe }
scheduling-answer-decline = { mcp-tool-value-decline }
scheduling-answer-comment = Note for the organizer (optional)
scheduling-answer-send = Send my answer to the organizer
scheduling-answer-confirm = Send “{ $answer }” to { $organizer }?
scheduling-answer-confirm-no-send = Only update my calendar
scheduling-answer-only-this = Answer only for this occurrence
scheduling-scope-legend = Answer for
scheduling-scope-series = { common-all-occurrences }
scheduling-scope-occurrence = One occurrence
scheduling-scope-day = Day of the occurrence

## Propose a new time

scheduling-propose-intro = Suggest another time to the organizer. Your calendar does not change.
scheduling-propose-start = Proposed start
scheduling-propose-end = Proposed end
scheduling-propose-send = Send proposal

## Forward

scheduling-forward-title = Forward invitation
scheduling-forward-to = To (addresses, separated by commas)
scheduling-forward-invalid = Enter at least one valid address.
scheduling-forward-comment = Note (optional)

## Attendee picker and free/busy

scheduling-attendees-legend = People and rooms
scheduling-attendees-none = No one is invited yet.
scheduling-attendee-me = (you)
scheduling-attendee-role = Role of { $email }
scheduling-attendee-invalid = This address is not valid.
scheduling-kind-individual = { common-person }
scheduling-kind-room = { invitation-attendee-room }
scheduling-kind-resource = { invitation-attendee-resource }
freebusy-label = Availability
freebusy-previous-day = Previous day
freebusy-next-day = Next day
freebusy-previous-short = Back
freebusy-empty = Add people to see when they are free.
freebusy-source-own = Your calendars
freebusy-source-caldav = Calendar server
freebusy-source-exchange = { common-exchange }
freebusy-source-unknown = No information
freebusy-kind-busy = busy
freebusy-kind-tentative = maybe busy
freebusy-kind-unavailable = out of office
freebusy-period = { $time } ({ $kind })
freebusy-row-loading = { $name }: loading
freebusy-row-error = { $name }: availability could not be checked
freebusy-row-unknown = { $name }: no availability information
freebusy-row-free = { $name }: free all day
freebusy-row-busy = { $name }: { $periods }
freebusy-meeting-free = Everyone is free at the chosen time.
freebusy-meeting-busy =
    { $count ->
        [one] Busy at the chosen time: { $names }
       *[other] { $count } people are busy at the chosen time: { $names }
    }
freebusy-suggestions = Times when everyone is free
freebusy-suggestions-none = No free time for everyone on this day.
freebusy-suggestion-pick = Move the meeting to { $day }, { $time }

## Find a time

scheduling-find-duration = Length
scheduling-find-create = Create meeting

## Organizer

scheduling-meeting-label = Meeting: { $title }
scheduling-meeting-organized-by = Organized by { $name }
scheduling-send-legend = Send invitations or the update to
scheduling-send-to-everyone = Everyone
scheduling-send-to-chosen = Only the people I choose
scheduling-send-invitations = Send invitations
scheduling-sent-invitations = The invitations are on their way.
scheduling-cancel-meeting = Cancel meeting…
scheduling-cancel-nothing = No meeting to cancel.
scheduling-cancel-intro = Cancel “{ $title }” and tell the attendees.
scheduling-cancel-legend = What to cancel
scheduling-cancel-scope-meeting = The whole meeting
scheduling-cancel-scope-occurrence = Only the occurrence on { $occurrence }
scheduling-cancel-scope-attendees = Remove some attendees
scheduling-cancel-people = Attendees to remove
scheduling-cancel-note = Note in the mail (optional)
scheduling-cancel-confirm = Cancel and notify
scheduling-keep-meeting = Keep the meeting
scheduling-cancelled = The meeting is cancelled; the attendees are told.

## Settings

scheduling-settings-legend = When invitations arrive
scheduling-setting-add-invitations = Put new invitations into my calendar
scheduling-setting-add-invitations-hint = They show as unanswered until you answer.
scheduling-setting-apply-updates = Apply updates from organizers
scheduling-setting-apply-updates-hint = Changed times and places update the event at once.
scheduling-setting-apply-replies = Record answers to my invitations
scheduling-setting-apply-replies-hint = Attendees' answers update my events at once.
scheduling-setting-apply-cancellations = Remove cancelled events
scheduling-setting-apply-cancellations-hint = Otherwise the invitation offers “Remove from calendar”.
scheduling-setting-confirm-replies = Ask before sending an answer
scheduling-setting-confirm-replies-hint = Accept, Maybe and Decline ask first whether to send.
scheduling-setting-calendar = Calendar for new invitations
