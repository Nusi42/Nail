# ADR 0073: Microsoft Graph engine and mock Graph server (WP91)

Status: accepted · 2026-10-01 · M3c WP91

## Context

Plan M3c WP91: `nail-proto-graph` syncs Exchange Online accounts through
Microsoft Graph `v1.0` against the contract of ADR 0072
(`nail_api::exchange`, `ExchangeEngine` = mail per folder + settings +
directory + PIM + free/busy + sending, the provider-neutral PIM shapes of
`nail_api::exchange::pim`). Graph change notifications need a public
webhook, so a desktop client polls deltas (plan "Scope decisions for M3").
No new third-party crates, no non-loopback sockets, no OpenSSL.

## Decisions

### Client (`client.rs`, `error.rs`, `batch.rs`, `wire.rs`)

- `GraphClient` on the workspace reqwest with nail-auth's rustls / ring
  configuration and no redirects. URL policy: `https:`, or `http:` to a
  loopback host only (the mock; `tls="plaintext"` on loopback). Every
  request must stay on the API's origin: `@odata.nextLink` /
  `@odata.deltaLink` values pointing elsewhere are refused
  (`GraphError::Protocol`), so the bearer token never leaves Graph; a
  stored cursor on another origin (or not a URL) is treated as expired.
- Auth: `GraphAuth::OAuth2 {tokens, provider, secret}` (the core's token
  manager, scopes of `mail_scopes(microsoft, [Graph])`) with one forced
  refresh on `401`, or a fixed bearer for tests. Provider errors pass
  through unchanged.
- **Throttling** (`RetryPolicy`, default 4 retries, 0.5 s doubling to
  16 s, 25 % jitter): `429`, `503`, `504` (and `500` / `502`, broken
  connections for idempotent calls) are retried inline honouring
  `Retry-After` (seconds or HTTP date); a wait above `max_inline_wait`
  (30 s) is not slept but returned as `ErrorCode::RateLimited` with
  `EngineError::retry_after_secs`, so `AdaptivePoll::failed` waits.
  Non-idempotent calls (`sendMail`, creates, `move` / `copy`, meeting
  answers, `cancel`) are repeated only after `429` (refused before
  processing), never after a `5xx`.
- Errors: `GraphError` → `ErrorCode` + retryable + `graph-*` Fluent ids
  (`locales/en/core-graph.ftl`, 19 ids, `MESSAGE_IDS` tested against the
  catalog). `410` and the codes `syncStateNotFound` / `resyncRequired` /
  `SyncStateInvalid` … are `SyncStateExpired` (→ `CursorExpired` when a
  cursor was used, an error on a first sync);
  `MailboxNotEnabledForRESTAPI` is `MailboxUnavailable` (`not_found`,
  own text: probably an on-premises mailbox); `409` / `412` are
  `Conflict`.
- **JSON batching** (`GraphClient::batch`): at most 20 requests per
  `$batch`, answers matched by id and returned in request order; parts
  answered `429` (any method) or `5xx` (not `POST`) go again in a later
  round after the longest part `Retry-After`; a wait beyond the inline
  limit leaves the throttled answers to the caller. Used for well-known
  folder ids, server info, mailbox probes, Focused Inbox changes, event
  reads, To Do checklist / link reads and checklist writes.

### Mail (`mail.rs`)

- **Mailboxes**: `None` = `/me`, `Some(address)` = `/users/{address}`
  (shared / delegate mailboxes; the server's permissions decide).
- **Hierarchy**: `mailFolders/delta` (`$select` of names, parents and
  counts), paged with `Prefer: odata.maxpagesize`; the deltaLink is the
  cursor. The well-known folders (`msgfolderroot`, `inbox`, `sentitems`,
  `drafts`, `deleteditems`, `junkemail`, `archive`, `outbox`) are resolved
  once per mailbox with one `$batch` (a missing Archive is fine) and give
  `ExchangeFolder::well_known`; children of the root have no parent; the
  root and hidden folders are left out. Graph v1.0 has no folder class:
  every folder is `Mail`.
- **Items**: `mailFolders/{id}/messages/delta` per folder with a fixed
  `$select` (ids, change key, conversation, folder, read, draft, flag,
  categories, Focused Inbox, received, `Message-ID`); `since` becomes
  `$filter=receivedDateTime ge …` on the first pass. At most
  `pages_per_call` (5 × 100) pages per call, then `more` with the
  nextLink as cursor. Graph does not tell new from changed messages:
  entries with their full metadata are `SyncDelta::added` (the core
  upserts by remote id), entries with some properties are `changed`,
  `@removed` ones (deleted or moved away) are `removed`. Focused Inbox is
  set only for Inbox messages; `ExchangeMessageInfo.mailbox` is the
  lowercase shared mailbox; `RemoteMessage.raw` stays `None`
  (`fetch_mime` = `$value`). `\Answered` is never reported (Graph has no
  such property in deltas; the core keeps its local state, as for
  Gmail).
- **Writes** (`RemoteWrite`): `SetFlags` → one `PATCH` of `isRead`,
  `flag.flagStatus`, `categories` (read, merged case-insensitively like
  Outlook, written only when they change) and, for `answered`,
  `PidTagLastVerbExecuted` = 102; an empty change sends nothing. `Move` →
  `POST move` (answers the new id), `Trash` → move to `deleteditems`,
  `Delete` → `permanentDelete`; a missing message is
  `WriteOutcome::Gone`. `Append` first looks for the `Message-ID` in the
  folder (a replayed append finds its copy), then uploads base64 MIME to
  `mailFolders/{id}/messages` and sets the flags. **MIME creates are
  drafts in Exchange** (Graph cannot clear `isDraft` for them): appends
  belong in Drafts; Exchange files sent mail itself.
- Extra (not in the trait): `copy_message`, `attachments` /
  `attachment_content` (`$value`), `create_folder` / `rename_folder` /
  `delete_folder`, `forget_well_known`.
- **Focused Inbox** (`classify`): batched `PATCH inferenceClassification`;
  with `always`, one `inferenceClassification/overrides` `POST` per
  distinct sender (Graph updates an existing override).
- **Sending** (`OutgoingTransport::send`, `send_mime(mailbox, …)`):
  `sendMail` with base64 MIME (`text/plain`). Envelope recipients missing
  from `To` / `Cc` / `Bcc` are added as a `Bcc` header; addresses that
  cannot go into a header as they are are refused (`unsupported`, no
  header injection). Uploads above 4 MB (after base64) are `too_large`
  (Graph's request limit; upload sessions for large attachments are a
  follow-up). Sending as a shared mailbox uses `/users/{mailbox}/sendMail`.

### Settings (`settings.rs`)

- Master categories: list / create (a `409` on replay answers the
  existing category of that name) / colour `PATCH` / delete (missing =
  done). **Graph cannot rename a category** (`displayName` is fixed):
  `change_category` creates the new name, deletes the old one and answers
  the new id; the trait's `update_category` (which answers `()`) does the
  same, so the store learns the new id from the next `categories()` read
  (contract request below).
- Automatic replies: `mailboxSettings/automaticRepliesSetting` read with
  `Prefer: outlook.timezone="UTC"`, written with a `PATCH` of
  `mailboxSettings`; replies as escaped simple HTML (`<br>` line breaks)
  and read back as text (`text::html_to_text`: tags dropped, blocks and
  `<br>` as line breaks, entities decoded, `script` / `style` / `head`
  dropped).
- Inbox rules: `mailFolders/inbox/messageRules` ↔ `ExchangeInboxRule`.
  Any set predicate / action outside the contract's subset (or
  `isReadOnly`) makes the rule `has_unsupported`; updating such a rule
  sends only `isEnabled`. `moveToFolder` / `copyToFolder` are **Graph
  folder ids** in `FolderId` at this seam (the core maps them to and from
  its folders). Rules sort by `sequence`; `hasError` → `in_error`.

### Directory and free/busy (`directory.rs`)

- Room lists / rooms through `places` (`microsoft.graph.roomlist`,
  `microsoft.graph.room`, `places/{list}/microsoft.graph.roomlist/rooms`),
  paged; device names and tags are the room's features. Without
  `Place.Read.All` Graph answers `403` → `forbidden` (the core shows
  `exchange-warning-rooms-need-consent`). `findRooms` is beta-only and not
  used.
- `check_mailbox`: one `$batch` of the user (display name, needs
  `User.ReadBasic.All`, optional) and the mailbox's Inbox (the access
  check: `403` → forbidden, `404` / `MailboxNotEnabledForRESTAPI` → not
  found).
- Free/busy: `calendar/getSchedule` in UTC, 20 addresses per call,
  `busy` / `tentative` / `oof` → `BusyKind` (free and working elsewhere
  are not busy), merged periods, per-address errors (`not_found` for
  unknown recipients) with `graph-*` texts; the source is
  `FreeBusySource::Exchange` of the engine's account
  (`GraphEngine::with_account_id`, set by `for_account`).
- `find_meeting_times` (`findMeetingTimes`, rooms as resource attendees)
  answers `MeetingSuggestion`s for the room finder / time picker.

### PIM (`pim.rs`)

- Folders: calendars (default → `default-calendar`, `canEdit` false →
  read-only, Graph's calendar colours mapped to the nearest category
  preset), the default contacts folder plus the top-level contact
  folders, and (own mailbox only — Graph has no To Do for other
  mailboxes) To Do lists (`defaultList` → `default-tasks`,
  `flaggedEmails` → `flagged-emails`, read-only).
- **The default contacts folder** is not listed by `contactFolders`; its
  id is the parent of a contact or of the listed folders. A mailbox with
  neither uses the alias `contacts` (`DEFAULT_CONTACTS`), read by a full
  listing and written through `/contacts`; the next folder listing
  answers the real id.
- **Calendars**: `calendarView/delta` over a window (default 365 days
  back, 730 ahead; `EngineOptions`) only says which events changed —
  occurrences point to their series. Changed singles and series masters
  are then read with `GET events/{id}` in a `$batch`, each with
  `Prefer: outlook.timezone="<originalStartTimeZone>"` so times keep the
  zone they were made in (custom zones read in UTC); a series gets its
  exceptions from `instances` over the window; cancelled occurrences come
  from `cancelledOccurrences` when the server returns it (otherwise they
  are not known — interop check). Events deleted between the delta and
  the read are reported removed. Changes outside the window are not seen.
- Event writes: `POST calendars/{id}/events` (Graph sends invitations for
  events with attendees: `MeetingSend::None` cannot be honoured for new
  meetings), `PATCH events/{id}` with `If-Match: W/"<changeKey>"` (`412`
  → `Conflict`, `404` → `Gone`), exceptions written by patching their
  occurrence (matched by original start date when the exception has no
  id yet) and cancelled occurrences deleted; delete = `cancel` for a
  meeting the user organizes when `send` is not `None`, else `DELETE`.
  Delete tells events from contacts by reading the id as an event first.
- Meeting answers: `accept` / `tentativelyAccept` / `decline` with
  `comment` and `sendResponse`; `propose_new_time` adds
  `proposedNewTime` to a tentative or declining answer; `event_instances`
  lists a series' occurrences.
- **To Do**: tasks are addressed through their list, so task remote ids
  are **`<list id>/<task id>`** (`todo_task_id`, `split_todo_task_id`;
  Graph ids never contain `/`). Deltas per list (`tasks/delta`), checklist
  items and linked resources per changed task through `$batch`; the
  task's `@odata.etag` is its change key (`If-Match`); writes sync the
  checklist (patch changed, create new, delete missing items).
- Contacts: delta per folder, `PATCH` with `If-Match`; Graph has no fax /
  "other" phone fields, groups or more than one web page (left out).

### Polling (`poll.rs`)

`PollTimer` wraps `AdaptivePoll` with the account's `poll_range()` over
whole rounds (hierarchy + folders + PIM): changes or a resync → minimum
interval, a quiet round → doubling to the maximum, errors → the largest
`Retry-After`, `more` → continue at once. `FolderPoll` spreads folders
over rounds: priority folders (Inbox) every round, a quiet folder every
2nd, 4th, … at least every 8th round, a change brings it back.

### Mock Graph server (feature `testing`)

`testing::MockGraphServer` binds 127.0.0.1:0 (ADR 0057) and serves an
in-memory `Tenant` under `/v1.0` (`/me`, `/users/{upn}`, `/places`,
`/$batch`): see the module docs for the routes. Delta queries share one
change log per mailbox (per collection: folders, a folder's messages, a
calendar, a contact folder, a To Do list); tokens are sequence numbers,
paging uses skip tokens, `expire_deltas` makes older links answer `410`.
Series appear in `calendarView/delta` as occurrences, `instances`
expands daily / weekly / absolute-monthly patterns, patching an
occurrence makes an exception, deleting one cancels it. Test controls:
`set_token`, `inject(InjectedError)` (recorded `429` / `503` / `500` /
`410` / `404` / `403` / `401` bodies, `Retry-After`, method / path
filters, counts — also inside batches), `set_request_budget` (`429`
beyond it), `forbid_places`, a request log (method, path, query, batch
part, token, `Prefer`, `If-Match`, content type, body), mutation helpers
(`deliver`, `change_message`, `move_message`, `delete_message`,
`add_folder`, `add_event` / `update_event` / `delete_event`,
`add_contact`, `add_task`, `set_schedule`, `with_mailbox`, `with_tenant`)
and inspection (`message`, `messages_in`, `sent`, `meeting_actions`,
`event(s)`, `contact`, `task`).

Fixtures (`crates/nail-proto-graph/fixtures/`): `tenant-demo.json`
(`ada@contoso.example` with folders, Focused / Other mail, a flagged
message with a category, a PDF attachment, categories, scheduled
automatic replies, an editable and an Outlook-only rule, a weekly series,
a single event and an invitation from Bob, contacts in two folders, To
Do lists with a checklist and a linked resource; the shared mailbox
`support@contoso.example`; a denied and an on-premises mailbox; three
rooms in two room lists; schedules) — `testing::fixtures::DEMO_TENANT`
for core tests and the demo-multi seed — and `responses/` with recorded
answers (a message rule, automatic replies, a schedule, a series event,
a To Do task, a contact, a delta page, a `$batch` answer, error bodies)
that the unit tests parse and the mock answers with.

### Dependencies

None added: reqwest, nail-auth, serde / serde_json, base64, chrono, url,
tokio, thiserror, tracing and axum (`testing`) were declared by WP90. MIME
header reading (`Message-ID`, recipients) is a small module of its own
(`mime.rs`) rather than a new mail-parser edge. No OpenSSL / aws-lc.

## Consequences

- WP95 builds the Exchange account loop on `GraphEngine::for_account`:
  hierarchy per mailbox, `folder_items` per folder with `FolderPoll` /
  `PollTimer`, store the deltaLink per folder, full resync on
  `CursorExpired`; replay ops as `RemoteWrite`s (store the new id after
  moves); sending through the engine (no Sent copy); PIM per collection
  with `pim_changes` / `write_pim`; free/busy and rooms; categories,
  automatic replies, rules through the settings trait; `server_info` for
  `TestAccount`.
- Contract follow-ups (WP95 / integrator): `update_category` should
  answer the new id (Graph renames by replacement); the rule seam carries
  Graph folder ids in `FolderId`; To Do task ids are composite.
- Not done: upload sessions for messages above 4 MB, immutable ids
  (`Prefer: IdType="ImmutableId"` would keep ids across moves — a
  possible later switch with a resync), `findRooms` (beta), nested
  contact folders (only top-level folders are listed), contact photos are
  read on demand only (`contact_photo`), the appended-draft limitation,
  `MeetingSend::None` for new meetings, calendar changes outside the
  window. Interop checks for the maintainer's tenant: `mailFolders/delta`
  returns nested folders, `cancelledOccurrences` on v1.0, `If-Match` with
  the event change key.
