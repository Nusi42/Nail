# ADR 0069: Invitations, scheduling and Gmail UI (WP84)

Status: accepted · 2026-10-01 · M3b WP84 (number to be assigned by the integrator)

## Context

Plan section 8, wave M3b, WP84: the invitation card in the reader, the
attendee picker with a free/busy timeline, organizer update / cancel
flows, Gmail label management and label-based views, Gmail categories as a
filterable property — against the contract of ADR 0065, in the feature
slots `ui/src/features/scheduling/` and `ui/src/features/gmail/`, with
realistic mocks, built in parallel with WP81–WP83. WP84 owned only those
two folders and `locales/{en,de}/{scheduling,gmail}.ftl`; everything else
it needs from shared files is listed for the integrator (WP85).

## Decisions

### Mount points instead of edits to shared components

The reader (`widgets/MessageView.tsx`) and the PIM event editor
(`features/pim/EventEditor.tsx`) belong to other owners, so the feature
exports components the integrator mounts:

| Component | Where | Props |
| --- | --- | --- |
| `InvitationCard` (`features/scheduling`) | reader, between the headers and the body | `messageId`, `invitation` (`MessageBody.invitation`), optional `attendeesOpen`, `showConflicts`; renders nothing without an invitation; fetches `GetInvitation` when the body has none |
| `AttendeePicker` | event editor, instead of its `AttendeeEditor` | `value`, `onChange`, `start`, `end`, `onTimeChange(start, end)` (suggested slot chosen), `organizer`, `eventId` (ignored in the user's own busy time), `identityId`, `nodeKey`, `hourStart`, `hourEnd`, `tz` |
| `MeetingPanel` | event editor, for a saved event (`mode: "edit"`) | `event`, `recurrenceId`; organizer: send invitations / update to everyone or chosen attendees with a note, "Cancel meeting…" (dialog); attendee: answer for the series or this occurrence, propose a new time; nothing for events without attendees or without the user |
| `MessageLabels` (`features/gmail`) | message list rows and the reader | `message: MessageSummary`; label chips (colours) and the category of Gmail mail, nothing for other mail |

The same features are also reachable on their own through layout widgets
(`invitation-card` = the selected message's card, `freebusy-timeline` =
"find a time", `label-list`, `category-tabs`), dialogs and actions, so a
layout can use them before the integrator mounts the components.

### Invitation card

- Shows what the core says (`Invitation`): kind (method + state), state
  against the calendars, issues — `sender-not-organizer` /
  `sender-not-attendee` first, as alerts —, time (viewer's zone, UI locale),
  repetition (the PIM recurrence summary), occurrence, place, organizer,
  the user's answer, call link, a reply's sender and answer, a
  counter-proposal, the sender's comment, attendees (collapsible
  `<details>`, counts per answer), conflicts with the user's calendars.
- Offers exactly the core's `actions`; nothing is decided in the UI.
  Answers carry an optional comment and a "send my answer" switch
  (`send: false` = only update the calendar); with
  `SchedulingSettings.confirm_replies` the card asks first (send / only
  update the calendar / back).
- Recurring invitations already in a calendar offer "all occurrences / one
  occurrence" with the PIM date picker; one occurrence answers through
  `InvitationTarget::Event` with the occurrence's `RECURRENCE-ID` (the
  series start's wall-clock time on the chosen day).
- "Propose a new time" uses the PIM `DateTimeInput` (React Aria fields
  under the UI locale, ADR 0062) — no native date inputs.

### Free/busy timeline and suggestions

- One `QueryFreeBusy` per visible day (whole day in the viewer's zone);
  rows show busy / tentative / out-of-office blocks, the source and
  per-attendee errors; the planned meeting is drawn over every row and the
  people busy then are named.
- Suggested slots are computed in the UI (`model.ts` `suggestSlots`): gaps
  of the meeting's length within the visible working hours, aligned to 15
  or 30 minutes, earliest first, never in the past. This is presentation
  over the core's answer (who is busy when stays the core's); a
  core-side "find a time for several people" can replace it later.
- Positions are data and go through theme geometry (`useGeometry`), never
  inline styles; `SCHEDULING_BASE_CSS` makes tracks positioning layers and
  stacks the busy kinds. Each track is `role="img"` with a text summary
  for screen readers.

### Gmail labels and categories

- Labels list: user labels as a tree (`/` nests; levels without a label of
  their own are plain text), list visibility honoured (`hide`,
  `show-if-unread`; widget option `show-hidden`), unread counts, pending
  sync marked. A **label view** is a search on the label's tag
  (`tag:"Name"`, account scope) — user labels are Nail tags (ADR 0065), so
  no new request is needed and the existing search list shows it.
- **Categories** are a search `category:<name>` scoped to the account's
  inbox (the query language gets `category:` in WP85); "All" clears it.
- Colours: the editor offers 27 named background / text pairs taken only
  from Gmail's API palette (`palette.ts`, full palette for checks), so
  Gmail never refuses them; a colour set elsewhere outside the palette
  stays selectable as "current colour".
- Editor validation mirrors `GmailLabelSettings::issues` plus duplicates
  for immediate feedback with the same `gmail-label-issue-*` texts; the
  core stays authoritative and its `localized` refusal is shown.
- Applying labels to a message is `UpdateFlags` with `add_tags` /
  `remove_tags` through the operation queue (optimistic list patch, the
  status bar's pending ops), not a new request.

### Mocks

`features/scheduling/mock.ts` is a small in-memory scheduler: cards by
message id with conflicts recomputed from the PIM mock's instances,
answers / counters / refreshes / forwards recorded in `state.sent`
(iMIP answers get an outbox id), updates / replies / cancellations
applied, organizer send / cancel (meeting, occurrence, attendees), counter
accept (moves the event) / decline, free/busy from the PIM mock plus demo
people (one with a network error), validation with the contract's
`freebusy-issue-*` ids. Calendar writes go through the PIM mock's own
requests (`answerPim`), like the core writes through the PIM queue.
`features/gmail/mock.ts` keeps labels per account with counts from the
messages, adopts unknown tags as labels, creates with `local-…` ids that
the "replay" turns into `Label_…` ids, renames nested labels and tags,
deletes the tag from messages. Both mocks keep an `unsupported` switch
(the core before WP85). Demo data comes from `seedSchedulingDemo` /
`seedGmailDemo` (`mockSeed.ts`), which the integrator calls from
`createMockData`; tests call them directly.

## Consequences

- Everything is themeable through `data-part`s; strings are in
  `scheduling.ftl` / `gmail.ftl` (German files stay empty); Vitest covers
  mocks, pure helpers, components and axe scans of every card, form,
  widget and dialog (67 + 40 tests).
- Not built here (listed for WP85): mounting the components, the
  nail-config option specs of the four widgets, the theme rules for the new
  parts, `category:` in `mockSearch.ts` / `nail-search`, the demo seeds in
  `createMockData`, `MessageBody.invitation` from the mock's `GetMessage`,
  undo-send for iMIP answers through the existing undo toast (the result
  carries `SentItip.outbox_id`), Gmail account setup in `AccountSetup`
  (OAuth with the Gmail API), Playwright scenarios.
