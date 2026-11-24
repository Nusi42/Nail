# 0248 — R7 quick look, compact editor, invitation card, scheduling lanes, reminder toast

Status: accepted (2026-11-24)

## Context

ADR 0246 scoped the R7 findings M4 (a click opens the full editor), S1
(unanswered invitations), S3 (the long editor form), S10 (the invitation
card), S11 (the scheduling grid) and C3 (the reminder toast) for the quick
look and editor package. This records how the package built them and where
it chose between options the contract left open.

## Decision

1. **Quick look content.** `EventPopover.tsx` reads the event with
   `GetEvent` (an override's fields for its occurrence) for place, people,
   organizer and call links; the instance alone gives the time. The user's
   answer comes from the event's own attendee entry, the instance's
   `my_status` until it loaded, so the quick look works before the core
   computes `my_status`. Edit has the focus: Enter edits from the first key
   press without a key handler of its own; Delete / Backspace delete from
   anywhere in the popover except text fields. Delete uses the calendar's
   `requestDelete` (the scope question for an occurrence, the queue's undo
   for a single event). An invitation is answered for the whole series
   (the editor's meeting panel answers one occurrence); overlaps are listed
   only for `needs-action`.
2. **One answer control.** `AnswerSegments` (toggle buttons in a group named
   "Your answer", the shared `segmented` look, Accept first) replaces the
   row of equal buttons in the card, the meeting panel and the quick look.
   The note for the organizer opens on "Add a note".
3. **Dates as text only in the event editor.** `DateTimeInput` gets
   `display="text"` (a button with "Fri, Nov 27, 2026" that opens the month
   grid); the default stays the numeric segments, because birthdays, task
   due dates and the occurrence picker are typed more often than picked.
   The end's day shows in the time row only when it differs from the
   start's; otherwise "End date" is under "More options".
4. **Chips are labelled selects.** Repeat, reminder, calendar and busy /
   free are native selects with visually hidden labels in a chip, so the
   keyboard, screen readers and existing tests keep working; the reminder
   chip offers none, at the start and common times before the start, and
   anything else is "Custom reminders" edited under "More options".
5. **The reader hides the iCalendar part when the card shows.** The card's
   own query (`useInvitationCard`, no extra request) decides, so a message
   whose card cannot load keeps its `.ics` attachment. No core flag.
6. **Scheduling lanes.** The meeting is one frame on the hour axis (a
   slider: arrow keys move it by 15 minutes, Shift by an hour) and in every
   lane (dragged with the pointer). People without shared availability are
   counted apart ("everyone with shared availability is free"), the chosen
   time is never suggested again, and "no time on this day" shows only when
   the chosen time is busy too, with "Try the next day". Busy blocks carry
   only how busy someone is — free/busy never has titles.
7. **Styles.** New parts are styled in `lookStyle.ts`'s skin; parts that
   `defaults/theme.kdl` already styles (the invitation card, the free/busy
   rows, the toast) are reached only from its base CSS through a parent
   part, so the default-theme fixture does not change. The quick look's
   root part stays `pim.popover` (the contract's slot) with its own look;
   buttons reuse `form.button` and its variants.

## Consequences

- Until the time grid package lands, a click on an event still opens the
  editor; the quick look's end-to-end scenario passes once a click opens
  it (ADR 0246 M4).
- `scheduling-attendee-role`, `freebusy-suggestions` and the conflict list's
  texts left `scheduling.ftl`; the card shows the overlap as a strip whose
  blocks name their time for screen readers.
