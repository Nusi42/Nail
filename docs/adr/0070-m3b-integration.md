# ADR 0070: M3b integration — invitations, scheduling, free/busy and Gmail accounts (WP85)

Status: accepted · 2026-10-01 · M3b WP85

## Context

Wave M3b built iTIP / iMIP (`nail-itip`, ADR 0066), the Gmail API engine
with its mock server (`nail-proto-gmail`, ADR 0067), CalDAV scheduling and
free/busy (`nail-dav`, ADR 0068) and the invitation / scheduling / Gmail UI
(ADR 0069) in parallel against the contract of ADR 0065. WP85 wires them
into the core, the demo, the default layout and the Playwright suite.

## Decisions

### Invitations (`nail-core/src/scheduling.rs`)

- **Cards**: `GetMessage` fills `MessageBody.invitation` (a broken iTIP part
  only loses the card) and `GetInvitation` answers the same:
  `nail_itip::find_itip` on the stored raw message, the user's addresses
  (`Service::me`: the receiving account's identities first, then every
  identity, then the accounts' addresses), the calendar copy found by `UID`
  (writable calendars first), the stored `DTSTAMP` of the applied
  organizer version (`itip_state` row of the organizer, recurrence `""`),
  the last reply of the replying attendee, `applied` from `itip_messages`,
  conflicts from the instance cache (`conflicts::window` /
  `invited_spans` / `find_conflicts`) and the transport
  (`server::attendee_transport` / `plan_organizer_delivery`).
- **Arrival**: the rule inbox pipeline (`process_rule_inbox`: security →
  junk → **iTIP** → rules → notifications) records every iTIP message
  (`itip_messages`) and applies it automatically when the card offers the
  matching action and the setting allows it (`add_invitations` for new
  `REQUEST`s to the user as attendee, `apply_updates`, `apply_replies`,
  `apply_cancellations`). Never for junk, never with
  `sender-not-organizer` / `sender-not-attendee` (state `ignored`),
  outdated messages are recorded as such. Mail of a folder's first sync
  is not an arrival (as for rules); its cards still work and offer the
  actions.
- **Writes** go through `save_item` / `delete_item` (PIM queue; local
  calendars stay local) with the text `nail-ical` writes by patching the
  stored original; a new event from an invitation is
  `nail_itip::apply::new_event` (the iTIP text without `METHOD` as the
  original). Versions are recorded in `itip_state` (organizer version,
  attendee answers).
- **iMIP mail** leaves through the outbox as a **prepared message**: the
  new `AutomaticMail::Itip` (contract addition) stores
  `OutgoingImip::to_mime()` as the draft's single `message/rfc822`
  attachment, sent unchanged like the Autocrypt setup message (no
  sign / encrypt, no Autocrypt header). Undo send applies
  (`SentItip.outbox_id`); every sent message is recorded in `itip_sent`.
  The readable texts come from `itip.ftl` in the core's locale.
- **RFC 6638 — never both**: the CalDAV discovery of a source now also
  asks `discover_scheduling` once and stores inbox / outbox / addresses /
  `auto_schedule` with the source's discovery JSON
  (`pim::engine::StoredScheduling`, `source_scheduling`). For a calendar
  on a server that schedules, Nail only writes the event (the attendee's
  `PARTSTAT` or the organizer's copy) and reports `transport: server`;
  attendees marked `SCHEDULE-AGENT=CLIENT` are mailed. The schedule inbox
  is read with every CalDAV pass (`process_schedule_inbox`): the server
  already changed the copies, so Nail records the versions, refreshes the
  cards and deletes the items.
- **Organizer side**: `CreateEvent` / `UpdateEvent` fill `organizer` for
  an event with attendees (the calendar's account identity, else the first
  identity) so the meeting panel and `SendInvitations` recognise the user.
  `SendInvitations` mails `REQUEST`s (an update when one was sent before),
  `CancelMeeting` cancels the meeting / one occurrence / some attendees and
  changes the copy, `RespondToCounter` accepts (`accept_counter`, then a
  `REQUEST` to everyone) or sends `DECLINECOUNTER`.
- **Free/busy** (`QueryFreeBusy`): the user's addresses get
  `FreeBusySource::Own` from `nail_dav::own_busy_periods` over the instance
  cache of the writable event calendars (the user's `PARTSTAT` decides
  tentative / ignored); the rest is asked through `CalDavFreeBusy` of every
  enabled CalDAV source with an outbox (the organizer is replaced by the
  principal's address when needed); a provider failure marks the attendees
  it was asked for; everyone else is `unknown`.

### Gmail accounts (`nail-core/src/gmail.rs`)

- **Engine loop**: `GmailEngine::for_account` with the core's token
  manager; each pass replays the label queue and the mail queue, upserts
  the six folders, applies the server's label list
  (`replace_gmail_labels`; `GmailLabelsChanged` when it changed), then a
  full listing (no `historyId`: pages stored as they come, an interrupted
  listing resumes from `full_sync_page`, the first page's cursor stored
  once all pages are in; a listing that started fresh removes local
  messages the server no longer lists) or history deltas until `more` is
  false (`CursorExpired` → full listing). `PollTimer` (plus
  `EngineTiming::poll_override`) decides the next pass; "Sync now" and
  queued writes wake it.
- **Storage**: one row per Gmail message, id derived from the Gmail id
  (`gmail_message_id`); known messages get metadata only (folder, flags,
  tags, Gmail data), new ones are fetched raw and ingested; a sync never
  clears a local `\Answered`. New mail of a delta goes to the rule inbox and
  `NewMail` (Inbox).
- **Replay** (`GmailExecutor`): flag / tag changes, moves (within the
  account), trash / delete and appends as `RemoteWrite`s by remote id;
  `Gone` drops the write; appends record the new remote id.
- **Sending**: `Service::transport` returns the Gmail engine for Gmail
  accounts (`OutgoingTransport`), `send_draft` checks `can_send()`, and no
  Sent copy is filed (Gmail files it; `saves_sent_automatically` covers
  every account that `sends_itself()`).
- **Labels**: the four requests validate (`GmailLabelSettings::issues`,
  duplicates as `gmail-label-issue-duplicate-name`), queue through
  `enqueue_gmail_label_op` and wake the engine.
- **Summaries** carry `MessageSummary.gmail` (attached with the tags in
  the store); `category:` is a query field (`nail-search` →
  `MessageFilter::GmailCategory`; rules see it through
  `MessageView::gmail_category`; IMAP server search ignores it).
- **Setup**: discovery offers the Gmail API first (new
  `DiscoverySource::Provider`) for `gmail.com`, `googlemail.com`, a Google
  host among the candidates or a Google MX (Workspace); IMAP proposals stay
  as alternatives. `TestAccount` signs in and reads `users.getProfile`.

### Demo and tests

- demo-multi: the mock CalDAV user's address is the work identity, a second
  principal `bob` with busy time, a meeting the work identity organizes;
  a Gmail account on `MockGmailServer::start_demo()` signed in through a
  `MockOAuthServer` (new `add_refresh_token`), **switched off** at first so
  the existing suites keep their counts. New controls: `gmail-deliver`,
  `gmail-message`, `gmail-modify`, `gmail-labels`, `gmail-sent`,
  `dav-user-put`.
- Core tests: `tests/scheduling.rs` (invitations added / answered / updated
  / cancelled, spoofing never applied, organizer invite / reply / counter /
  cancel, free/busy own + CalDAV + unknown, settings, server scheduling
  with no iMIP), `tests/gmail.rs` (sync, labels, categories, replay,
  sending, history, expiry resync, label queue, `TestAccount`),
  `tests/m3b.rs` (every request served).
- Playwright: `scheduling.spec.ts` (accept + REPLY on the mock SMTP server +
  calendar, update and cancel, counter-proposal, free/busy from bob) and
  `gmail.spec.ts` (sync, categories, label, archive, send, history
  polling); the a11y sweep covers the six new dialogs, the meeting panel and
  the Gmail views.

## Deviations

- Gmail thread ids are not used as Nail thread ids: Gmail messages are
  threaded by their headers like all mail (the Gmail thread id is stored in
  `gmail_messages`).
- Raw messages of a listing are fetched one by one (`fetch_raw`); a
  headers-only policy for Gmail is not applied yet.
- Schedule-inbox items are acknowledged and removed, not applied by Nail
  (the server applied them to the copies); a server that only delivers to
  the inbox without updating calendars would need Nail to apply them.
- The demo Gmail account starts switched off (see above).

## Consequences

Every M3b request is served; the plan's WP85 Playwright scenarios run in
`npm run e2e`. Follow-ups are listed in `docs/plan.md` ("M3b follow-ups").
