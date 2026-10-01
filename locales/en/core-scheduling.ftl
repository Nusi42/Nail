# Nail — English texts about invitations and scheduling built in Rust
# (M3b; contract ADR 0065): problems with invitation messages (ItipIssue),
# free/busy questions (FreeBusyIssue) and, from WP81, the texts of the
# iMIP mails Nail sends (reply subjects and bodies).
#
# Owned by WP81 (nail-itip). The ids named by the contract are pre-filled;
# nail-api/tests/catalog.rs checks them.

## Problems with invitation messages (ItipIssue)

itip-issue-unparsable = The calendar data in this message cannot be read.
itip-issue-unknown-method = This message contains calendar data of an unknown kind.
itip-issue-no-event = This message contains no event.
itip-issue-missing-uid = The event in this message has no identifier.
itip-issue-missing-organizer = The invitation does not say who organizes the event.
itip-issue-attendee-count = The reply must name exactly one participant.
itip-issue-sender-not-organizer = This invitation was not sent by the organizer. Check it before you answer.
itip-issue-sender-not-attendee = This reply was not sent by the participant it names.
itip-issue-not-addressed-to-me = None of your addresses is invited to this event.
itip-issue-several-events = This message contains several events; only the first is shown.
itip-issue-organizer-changed = This message names a different organizer than the event in your calendar. It may not come from the real organizer.
itip-issue-sent-on-behalf = This message was sent by someone acting for the organizer or participant. Check it before you answer.
itip-issue-uninvited-attendee = This answer comes from someone who is not invited to the event.

## Problems with free/busy questions (FreeBusyIssue)

freebusy-issue-attendee-count = Choose between 1 and 100 people.
freebusy-issue-invalid-attendee = An address is not valid.
freebusy-issue-invalid-range = The time range is not valid.
freebusy-issue-range-too-long = The time range can be at most 92 days.
