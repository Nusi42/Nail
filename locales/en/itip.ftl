# Nail — English texts of the invitation mails Nail sends (iMIP, RFC 6047;
# M3b WP81, crate nail-itip). The readable part next to the calendar data:
# subject, an introduction, label / value lines and a footer.
#
# Arguments: $title (event title), $organizer / $attendee (names or
# addresses), $status (accepted, tentative, declined, delegated,
# needs-action), $date (the occurrence), $delegate (an address).
# Times are passed preformatted (2026-10-05 09:00).

## Subjects

itip-subject-request = Invitation: { $title }
itip-subject-update = Updated invitation: { $title }
itip-subject-publish = Event: { $title }
itip-subject-add = More dates: { $title }
itip-subject-cancel = Cancelled: { $title }
itip-subject-cancel-occurrence = Cancelled: { $title } on { $date }
itip-subject-reply =
    { $status ->
        [accepted] Accepted: { $title }
        [tentative] Tentatively accepted: { $title }
        [declined] Declined: { $title }
        [delegated] Delegated: { $title }
       *[other] Answer: { $title }
    }
itip-subject-counter = New time proposed: { $title }
itip-subject-decline-counter = Proposal declined: { $title }
itip-subject-refresh = Please send the latest version: { $title }

## Introductions

itip-body-request = { $organizer } invites you to “{ $title }”.
itip-body-update = { $organizer } has changed “{ $title }”.
itip-body-publish = { $organizer } shares the event “{ $title }”.
itip-body-add = { $organizer } has added dates to “{ $title }”.
itip-body-cancel = { $organizer } has cancelled “{ $title }”.
itip-body-cancel-occurrence = { $organizer } has cancelled “{ $title }” on { $date }.
itip-body-reply =
    { $status ->
        [accepted] { $attendee } has accepted the invitation to “{ $title }”.
        [tentative] { $attendee } has tentatively accepted the invitation to “{ $title }”.
        [declined] { $attendee } has declined the invitation to “{ $title }”.
       *[other] { $attendee } has answered the invitation to “{ $title }”.
    }
itip-body-reply-delegated = { $attendee } has passed the invitation to “{ $title }” on to { $delegate }.
itip-body-counter = { $attendee } proposes a new time for “{ $title }”.
itip-body-decline-counter = { $organizer } has declined your proposal for “{ $title }”.
itip-body-refresh = { $attendee } asks for the latest version of “{ $title }”.

## Detail lines

itip-label-when = When
itip-label-where = Where
itip-label-join = Join
itip-label-organizer = Organizer
itip-label-attendees = Participants
itip-label-proposed = Proposed time
itip-label-comment = Comment
itip-label-description = Details
# One line of the plain-text mail.
itip-line = { $label }: { $value }

## Times

itip-when-range = { $start } – { $end }
itip-when-zone = { $time } ({ $zone })
itip-when-all-day = { $date } (all day)
itip-when-all-day-range = { $start } – { $end } (all day)
itip-when-recurring = { $when }, repeating

## Answers of participants

itip-status-accepted = accepted
itip-status-tentative = tentative
itip-status-declined = declined
itip-status-delegated = delegated
itip-status-needs-action = no answer yet

## Other

itip-untitled = (No title)
itip-footer = This message contains calendar data. Calendar programs can add the event to your calendar and send your answer.
