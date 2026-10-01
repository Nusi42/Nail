# ADR 0065: M3b contract — invitations, scheduling, free/busy, Gmail API accounts, store v11, slots

Status: accepted · 2026-10-01 · M3b WP80

## Context

Wave M3b (plan section 8) adds iTIP invitations over iMIP mail and CalDAV
auto-scheduling (WP81, `nail-itip`), the Gmail API engine with a mock
Gmail server (WP82, `nail-proto-gmail`), CalDAV scheduling and free/busy
on the client and the mock DAV server (WP83, `nail-dav`,
`nail-testkit-net`) and the invitation / Gmail UI (WP84), built in
parallel and integrated by WP85. This ADR fixes the contract they build
against, the store schema they share and the slots that keep them apart.

Constraints (plan "Scope decisions for M3"): no GPL / LGPL / AGPL
dependency, no OpenSSL / aws-lc; no non-loopback sockets (ADR 0057; the
mock Gmail server binds 127.0.0.1); no push for Gmail (history polling
with adaptive intervals); Google calendars and contacts stay on CalDAV /
CardDAV, only mail moves to the Gmail API; strings only from Fluent
catalogs; the core owns data and every write goes through a queue.

## Decisions

### Invitations and scheduling (`nail_api::scheduling`)

- **Methods** `ItipMethod`: `publish`, `request`, `reply`, `add`,
  `cancel`, `refresh`, `counter`, `decline-counter` (iCalendar names via
  `as_ical` / `from_ical`; `from_organizer` tells the direction).
  **Attendee states** stay `pim::ParticipationStatus` (`PARTSTAT`);
  answers are `InvitationResponse` (`accept` / `tentative` / `decline`).
- **The invitation card** is `Invitation` on
  `MessageBody.invitation` (optional; also `GetInvitation`): method,
  `UID`, recurrence id, `SEQUENCE`, `DTSTAMP`, the user's **role**
  (`organizer` / `attendee` / `observer`), the **state** against the
  calendars (`new`, `current`, `update`, `outdated`, `cancelled`,
  `unknown-event`, `invalid`), organizer, attendees, the user's address,
  **my status** (from the calendar copy, else the message), the replying
  identity, the attendee a `REPLY` / `COUNTER` / `REFRESH` came from, the
  event's fields and recurrence, a `CounterProposal`, the sender's
  `COMMENT`, the user's event / calendar, **conflicts**
  (`InvitationConflict`: overlapping busy instances, tentative flag), the
  **actions** the card may offer (`InvitationAction`), the
  **transport** (`SchedulingTransport`: `imip`, `server` = RFC 6638
  auto-scheduling, `none`) and **issues** (`ItipIssue`, Fluent ids
  `itip-issue-*`; `sender-not-organizer` / `sender-not-attendee` warn
  about spoofing and block automatic processing).
- **Targets**: requests that answer act on `InvitationTarget::Message`
  (the card) or `InvitationTarget::Event` (an event already in a
  calendar, e.g. from the editor), so answering does not need the mail.
- **Requests** (12): `GetInvitation`; attendee side
  `RespondToInvitation {target, response, comment?, send, calendar_id?}`
  (`send: false` = "don't send a response"), `ProposeNewTime` (`COUNTER`),
  `RefreshInvitation`, `ForwardInvitation {to}` (RFC 5546 §2.1.5, the
  original `REQUEST`); `ApplyInvitation {message_id, calendar_id?}`
  (add / update / cancel / record a reply / answer a refresh without
  answering); organizer side `SendInvitations {event_id, recurrence_id?,
  attendees}` (after `CreateEvent` / `UpdateEvent`; `SEQUENCE` goes up on
  significant changes), `CancelMeeting {scope: meeting | occurrence |
  attendees}`, `RespondToCounter {accept}` (accept = move + update to all,
  decline = `DECLINECOUNTER`); `QueryFreeBusy`; `Get` /
  `SetSchedulingSettings`. Scheduling writes answer
  `Response::Scheduled {result: SchedulingResult}` (the event after the
  change or the removed id, the `SentItip`s with outbox ids — undo send
  applies to iMIP mail —, PIM op ids, the refreshed card).
- **Settings** (`SchedulingSettings`, stored in the app settings):
  `add_invitations` (default on: new invitations appear unanswered),
  `apply_updates`, `apply_replies` (on), `apply_cancellations` (off: the
  card offers "Remove from calendar"), `calendar_id`, `confirm_replies`.
- **Free/busy**: `QueryFreeBusy {attendees, from, to, identity_id?,
  ignore_event_id?}` → `FreeBusyResult` with one `AttendeeFreeBusy` per
  attendee in request order: `source` (`own` = the user's calendars,
  `caldav {source_id}` = the schedule outbox, `exchange {account_id}`
  reserved for M3c, `unknown`), merged `BusyPeriod`s (`BusyKind`
  `busy` / `tentative` / `unavailable`; `BusyPeriod::merge`), or an error
  per attendee. Limits: 100 attendees, 92 days (`FreeBusyIssue`,
  `freebusy-issue-*`). Rooms / resources are attendees with `CUTYPE`
  `room` / `resource`; their lookup arrives with Exchange (M3c).
- **Events**: `InvitationsChanged {message_ids}`,
  `SchedulingSettingsChanged`.
- Answering sends iMIP mail **through the outbox** of the identity the
  card names (so sending goes through the queue and undo send works) or,
  with `server` transport, only writes the event (the server mails; no
  duplicates). Event changes go through the PIM queue.

### Gmail accounts (`nail_api::gmail`, `accounts.kdl`)

- **Account kind**: `IncomingServer::Gmail {server, options:
  GmailOptions}` → `AccountKind::Gmail`, `MailProtocol::Gmail` (default
  port 443). In `accounts.kdl` a `gmail` node (host defaults to
  `gmail.googleapis.com`; `poll-interval` ≥ 10 s, default 30;
  `max-poll-interval`, default 300) with `auth "oauth2"` (required;
  `config-gmail-needs-oauth2`); `tls="plaintext"` only for loopback hosts
  (mock server); no `smtp` (`config-gmail-no-smtp`): Gmail sends through
  `users.messages.send` (`IncomingServer::sends_itself`,
  `AccountConfig::can_send`). The built-in `google` provider's scope
  `https://mail.google.com/` covers the API. Reader, format-preserving
  writer, JSON Schema and `userDocs/en/accounts-reference.md` updated.
- **Storage model**: a Gmail message is stored **once** (Gmail ids are
  per message). Its folder follows from its labels
  (`GmailFolder::for_labels`: Trash > Spam > Drafts > Inbox > Sent >
  Archive = "All Mail"); moves are label changes
  (`GmailFolder::move_change`; drafts and Sent → Archive are not moves).
  `UNREAD` ↔ not `\Seen`, `STARRED` ↔ `\Flagged`
  (`LabelChange::from_flags`). **User labels are Nail tags** named like
  the label (rules, search `tag:`, the tag UI keep working; renaming a
  label renames the tag; a new tag on a Gmail message creates the label).
  **Categories** are `GmailCategory` on `MessageSummary.gmail`
  (`GmailMessageInfo {gmail_id, thread_id, label_ids, category}`), a
  filterable property (query `category:<name>` in the query language,
  added by WP85 in `nail-search`). **Gmail thread ids** are the Nail
  thread ids of Gmail messages.
- **Label management**: `ListGmailLabels` → `GmailLabels` (system labels
  first, counts from the stored label ids), `CreateGmailLabel` /
  `UpdateGmailLabel` → `GmailLabelSaved {label, op_ids}`,
  `DeleteGmailLabel` → `Done`; `GmailLabelSettings::issues`
  (`gmail-label-issue-*`). Event `GmailLabelsChanged {account_id}`.
  Writes go through a per-account **label operation queue**
  (`gmail_label_ops`); a created label has a `local-…` id until the
  server assigned one. Gmail accepts only colours of its palette; others
  come back as `rejected` (WP84 offers the palette).

### Provider-neutral engine additions (`nail_api::engine`)

API engines differ from IMAP in four ways, so the contract adds (Rust
only, documented in the module):

- `MailApiEngine` (object safe): `folders`, `full_sync_page(page,
  since)` (the first page carries the cursor taken before listing),
  `changes(cursor)` → `DeltaOutcome::Delta(SyncDelta {cursor, added,
  changed, removed, more})` or `CursorExpired` (Gmail: `historyId` too
  old → full resync), `fetch_raw(remote_id)`, `write(RemoteWrite)`
  (`SetFlags` / `Move` / `Trash` / `Delete` / `Append`, idempotent) →
  `WriteOutcome::Done {remote_id}` / `Gone`. Sending stays
  `OutgoingTransport`. M3c's Graph and EWS engines implement the same
  trait.
- **Remote ids**: `messages.remote_id` (unique per account, kept across
  moves) and `MessageSource.remote_id` (filled by `Store::enqueue`), so
  queued writes replay without UIDs.
- `AdaptivePoll` (min / max interval, doubles when quiet, resets on
  activity, honours `Retry-After` once) — no push on the desktop.
- `EngineError::retry_after_secs` + `EngineError::rate_limited` (quotas,
  HTTP 429 / 503).
- `FreeBusyProvider` (CalDAV schedule outbox in WP83, Exchange in M3c).

### Remote access

`nail-remote-auth::required_access`: cards and free/busy are `read`,
answering / organizing / applying `calendar` (the core also needs `send`
when it mails), scheduling settings `admin`, Gmail labels `organize`.
The core (`check_remote_request`) treats scheduling like PIM (a grant for
every account) plus the message a card came in (granted and readable);
Gmail label requests need their account. `InvitationsChanged` /
`SchedulingSettingsChanged` go to full grants, `GmailLabelsChanged` to
grants of the account.

### Served now

Nothing: the core routes every M3b request to
`nail-core/src/scheduling.rs` / `gmail.rs`, which answer `unsupported`
until WP85; a Gmail account's engine reports itself offline with
`unsupported`; `MessageBody.invitation` and `MessageSummary.gmail` stay
empty. `tests/m3b.rs` checks all 16 requests, the Gmail account kind and
status; `tests/remote.rs` the remote rules. The UI mocks
(`features/scheduling/mock.ts`, `features/gmail/mock.ts`) answer the
same.

### Store schema v11 (`nail-store`)

`messages.remote_id` (+ unique index per account); `itip_state` (per
`UID` + recurrence id + address: role, `SEQUENCE`, `DTSTAMP`, `PARTSTAT`,
last message; only moves forward — `scheduling::itip_is_newer`: higher
`SEQUENCE`, then later `DTSTAMP`); `itip_messages` (received iTIP
messages: method, `UID`, sequence, sender, state `new` / `applied` /
`outdated` / `ignored` / `failed`, the event); `itip_sent` (replies,
requests, cancels, counters, refreshes Nail sent: transport, recipients,
outbox id); `gmail_sync` (`historyId`, interrupted full sync);
`gmail_labels` (id ↔ name ↔ tag / folder, colours, visibility, `synced` /
`pending`); `gmail_messages` (Gmail thread id, label ids, category);
`gmail_label_ops` (the label queue). Nothing new is sealed (comments are
not stored; label names are like tags). Store APIs are part of the
contract (`src/scheduling.rs`, `src/gmail.rs`, with tests): scheduling
state / messages / sent records / settings; remote ids; Gmail sync state,
labels (`replace_gmail_labels` renames / removes tags,
`list_gmail_labels` with counts), per-message data, the label queue
(`enqueue_gmail_label_op` applies locally in the same transaction,
`finish_gmail_label_op` moves a created label to the server id).
Parallel packages add no migrations; WP85 appends v12 if needed.

### Crates and libraries

No new third-party crate: `nail-itip` uses the workspace mail-parser /
mail-builder (Apache-2.0 OR MIT) and chrono with `nail-ical`;
`nail-proto-gmail` the workspace reqwest (TLS from `nail-auth`, ring),
serde_json, base64, chrono and, behind its `testing` feature, axum (MIT)
for the mock server. Both stubs build on Windows MSVC and have library
smoke tests (an iMIP mail built and parsed, its `METHOD` read by
`nail-ical`; the mock server answering `users.getProfile` over loopback
through the nail-auth HTTP client, base64url, the label mapping).
`cargo deny` / `cargo tree -i openssl-sys` / `aws-lc-rs` unchanged.

### Collision-free slots

| Area | Slot | Owner |
| --- | --- | --- |
| iTIP / iMIP, organizer / attendee logic, sequence / DTSTAMP, conflicts, RFC 6638 detection | `crates/nail-itip/`, `crates/nail-ical/` (iTIP needs), `locales/{en,de}/core-scheduling.ftl` | WP81 |
| Gmail API engine, mock Gmail REST server (feature `testing`, fixtures) | `crates/nail-proto-gmail/`, `locales/{en,de}/core-gmail.ftl` | WP82 |
| CalDAV scheduling (outbox / inbox, VFREEBUSY), free/busy of own calendars, mock DAV scheduling, room placeholders | `crates/nail-dav/` (`src/scheduling.rs` created), `crates/nail-testkit-net/`, `crates/nail-store/src/pim/freebusy.rs` (created), `locales/{en,de}/core-dav.ftl` | WP83 |
| Invitation card, attendee picker / free/busy timeline, organizer flows, Gmail labels / categories UI | `ui/src/features/scheduling/`, `ui/src/features/gmail/` (registered, mocks routed, state `data.features.{scheduling,gmail}`, messages in the coverage test), `crates/nail-config/src/widgets/{scheduling,gmail}.rs` (future widgets `invitation-card`, `freebusy-timeline`, `label-list`, `category-tabs`), `locales/{en,de}/{scheduling,gmail}.ftl` | WP84 |

Integrator-owned during M3b (report, do not edit): `Cargo.toml` (ask
before adding dependencies), `Cargo.lock` beyond a package's own new
dependency, `nail-api`, `nail-store` (except `src/pim/freebusy.rs`),
`nail-core`, `nail-server`, `nail-mcp`, `nail-search`,
`nail-remote-auth`, `nail-config` (except the two widget files),
`app/src-tauri`, `defaults/`, `e2e/`, `docs/adr` numbering. Contract or
schema changes go to WP85 as `apiRequests`.

## Consequences

- WP81–WP84 compile against fixed types and generated TypeScript; the
  suites stay green in between.
- WP85 wires: iMIP detection on arrival (`itip_messages`, automatic
  processing per `SchedulingSettings`, spoofing issues block it), the
  card on `GetMessage`, answering / organizing through the outbox and
  the PIM queue (or server scheduling), free/busy (own calendars + CalDAV
  outbox), Gmail engines (`MailApiEngine` sync loop with `AdaptivePoll`,
  op replay with remote ids, the label queue, sending, folders from
  `GmailFolder`, thread ids, categories in search), account setup and
  `TestAccount` for Gmail, demo-multi with a mock Gmail account and DAV
  scheduling, and the M3b Playwright scenarios.
- Exhaustive matches (core dispatch, `check_remote_request`,
  `remote_event`, `required_access`) fail to compile when a request or
  event is added; `SCHEDULING_REQUESTS` (12) and `GMAIL_REQUESTS` (4) in
  the UI mocks and `tests/m3b.rs` (16) count them.
