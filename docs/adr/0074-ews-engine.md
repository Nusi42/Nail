# ADR 0074: EWS engine and mock EWS server

Status: accepted · 2026-10-01 · M3c WP92

## Context

Plan M3c WP92: `nail-proto-ews` talks Exchange Web Services to Exchange
Server on-premises (2016, 2019, Subscription Edition) and implements
`nail_api::exchange::engine::ExchangeEngine` (ADR 0072): mail per folder
with sync states, push through streaming notifications, settings
(categories, automatic replies, inbox rules), the directory (room lists,
rooms, other mailboxes), PIM (calendars, contacts, tasks, meeting
responses), free/busy and sending. Windows sign-in is injected
(`NegotiateAuth`, SSPI in `nail-exchange`, WP93); Autodiscover is WP93's.
No new third-party crates beyond the workspace; no non-loopback sockets.

## Decisions

### SOAP and XML (`soap.rs`, `xml.rs`)

- Bodies are written as strings with escaping (`xml::escape` drops
  characters XML 1.0 cannot carry) and read into a small tree on
  `quick-xml` (`NsReader`, DOCTYPE refused, depth / element limits),
  matched by **local name** (servers differ in prefixes; `m:` and `t:`
  never share a local name inside one answer). The tree serializes back,
  for inbox rules written back verbatim and the category list document.
- `RequestServerVersion` is **`Exchange2016`** (all supported servers
  accept it; the stub's `Exchange2013_SP1` is the fallback): a fault
  `ErrorInvalidServerVersion` switches the client to `Exchange2013_SP1`
  once and repeats the call. `ServerVersionInfo` is kept and reported as
  `Exchange 2019 (15.2.1544.4)` in `ExchangeServerInfo::server_version`.
- `TimeZoneContext` is sent for writes with wall times (events, tasks in a
  Windows zone). `X-AnchorMailbox` names the mailbox a call is about (the
  own address by default).
- Errors: SOAP faults (detail `ResponseCode`, or the code in
  `faultcode`), per-message `ResponseClass="Error"` codes and HTTP statuses
  map to `EwsError` → `ErrorCode` + retryable + `retry_after_secs` + an
  `ews-*` text (`locales/en/core-ews.ftl`, 18 ids, catalog checked by a
  unit test).

### Client and sign-in (`client.rs`, `auth.rs`)

- URL policy as for Gmail / DAV: `https:`, or `http:` to loopback only.
- **Lanes**: one reqwest client per lane (`pool_max_idle_per_host(1)`,
  HTTP/1.1 only — Negotiate does not work over HTTP/2). The main lane
  carries calls; streaming notifications get their own lane (a waiting
  `GetStreamingEvents` never blocks sync, and NTLM's connection binding
  is per lane).
- **Negotiate**: the lane is locked for the whole request; an unsigned
  lane starts the handshake pre-emptively (`step(None)` token on the first
  request, the server's `WWW-Authenticate: Negotiate <token>` fed back,
  at most six legs, the final token of a `2xx` checked for mutual
  authentication); a signed lane sends no header, and a `401` there (the
  server replaced the connection) runs one new handshake. SPN
  `HTTP/<host>` (`http_spn`).
- **Basic** (only over TLS by the URL policy) with the keychain password
  read once and re-read once after a `401`; **OAuth2** bearer with one
  forced refresh after a `401` (scope `EWS.AccessAsUser.All`, the core's
  token manager).
- **Throttling**: `ErrorServerBusy` (fault or response message) with
  `BackOffMilliseconds` is slept inline up to `max_inline_wait` (30 s,
  `RetryPolicy` like Gmail's), else answered as `RateLimited` with the
  wait (`retry_after_secs`, rounded up). HTTP `429` / `5xx` honour
  `Retry-After`; non-idempotent writes are not repeated after a broken
  connection or a `5xx`.

### Mail (`mail.rs`, `engine.rs`)

- `folder_hierarchy`: `SyncFolderHierarchy` below `msgfolderroot`;
  well-known folders by one `GetFolder` of the distinguished ids per
  mailbox (cached; Archive by name below the root); children of the root
  have no parent; `FolderClass` → mail / calendar / contacts / tasks /
  other. `ErrorInvalidSyncStateData` → `CursorExpired`.
- `folder_items`: `SyncFolderItems` (up to 512 changes per call → `more`)
  with an `IdOnly` shape plus `ParentFolderId`, `DateTimeReceived`,
  `Size`, `Categories`, `IsDraft`, `Flag`, `ConversationId`, `IsRead`,
  `PR_LAST_VERB_EXECUTED` (answered) and, only with
  `EwsOptions::focused_inbox`, `InferenceClassification` (on-premises
  servers have no Focused Inbox and older schemas refuse the field).
  `ReadFlagChange`s are completed by one batched `GetItem`. `since`
  filters the first pass by `DateTimeReceived`. `RemoteMessage::folder` is
  the folder **as the caller named it** (an id, or a distinguished name).
- MIME through `GetItem` `IncludeMimeContent` (base64).
- Writes: flags absolute (`AlwaysOverwrite`, no change key needed);
  **tag changes read the categories and change key first and write with
  `NeverOverwrite`**, retrying twice on `ErrorIrresolvableConflict`
  (categories merge case-insensitively). `Move` / `Trash` are `MoveItem`
  (`ReturnNewItemIds`) and answer the new id; `Delete` is `HardDelete`;
  `Append` is `CreateItem` `SaveOnly` with `MimeContent`,
  `PR_MESSAGE_FLAGS` (so it is not a draft) and is idempotent through
  `FindItem` on `InternetMessageId`. A missing item is `Gone`.
- Sending: `CreateItem` `SendAndSaveCopy` into Sent Items
  (`EwsOptions::save_sent_copy`, from the account's `save-sent-copy`);
  envelope recipients missing from the headers become a `Bcc` header
  (Exchange sends to the MIME recipients).
- `classify`: `UpdateItem` of `InferenceClassification` when the server
  has Focused Inbox; `always` (sender overrides) is `unsupported` — EWS
  has no API for overrides (Graph does).

### Streaming notifications (`notify.rs`)

`wait_for_changes` (`StreamingWatcher`) keeps one streaming subscription
per mailbox for the folders asked (re-subscribing, and unsubscribing the
old one, when the set changes; distinguished names resolved to ids),
opens `GetStreamingEvents` with a `ConnectionTimeout` covering the wait,
and answers the watched folders named in events (parent and old parent
folders) — empty at the timeout. A server close (`ConnectionStatus
Closed`) reopens the stream; a broken stream reconnects (up to three
times per wait, with backoff). A subscription the server forgot
(`ErrorSubscriptionNotFound` & co.) is renewed and **every watched folder
is reported as changed** (events in between are lost). With
`streaming = false` the engine answers `None` (the core polls).

### Settings, directory, PIM

- Automatic replies: `Get/SetUserOofSettings` of the own address; replies
  are written as simple HTML (`html::text_to_html`) and read as text
  (`html_to_text`: blocks → lines, entities, `<head>` / `<style>` dropped).
- Inbox rules: `GetInboxRules` / `UpdateInboxRules`. Rules with
  `IsNotSupported` or conditions / actions outside the contract's subset
  are `has_unsupported`; changing them writes the server's own `t:Rule`
  XML back with only `IsEnabled` replaced. `UpdateInboxRules` answers no
  id, so a created rule is found again by name, conditions and actions.
  `RemoveOutlookRuleBlob` is **false** by default: a mailbox with Outlook
  client-only rules answers `Conflict` (`ews-outlook-rule-blob`) instead
  of losing them (`EwsOptions::remove_outlook_rules`). At the engine seam
  `move_to_folder` / `copy_to_folder` carry folder **remote ids**.
- Categories: the `CategoryList` user configuration on the calendar
  (`XmlData`, base64 XML). Edits keep Outlook's other attributes; new
  categories get a random GUID; a mailbox without the list gets
  `CreateUserConfiguration`.
- Rooms: `GetRoomLists`, `GetRooms` (no capacity / building in EWS);
  `rooms(None)` merges every list. Free/busy: `GetUserAvailability` in a
  UTC `TimeZone` (answers are UTC), 100 mailboxes per call, rooms asked as
  `Room`, `Free` / `WorkingElsewhere` / `NoData` dropped, `OOF` →
  `Unavailable`; unknown attendees (`ErrorMailRecipientNotFound`) are left
  out, other failures carry an error code. `check_mailbox`: `GetFolder` of
  the other mailbox's root (access) and `ResolveNames` (name).
  `get_delegates` reads `GetDelegate`.
- PIM folders from the hierarchy (classes), roles from the well-known
  folders; a delegate without access to the hierarchy falls back to the
  distinguished calendar / contacts / tasks of that mailbox. Changes:
  `SyncFolderItems` (`IdOnly`) + `GetItem` with the kind's properties;
  series get their changed occurrences (`ModifiedOccurrences` → `GetItem`,
  `series_master_id` set) and `DeletedOccurrences` as dates.
- **Times**: EWS answers in UTC, so timed events and tasks come back as
  `ExchangeDateTime`s in `UTC` (the mapping layer converts; the series
  zone is `recurrence.time_zone` from `StartTimeZone`). All-day events are
  shifted by the `StartTimeZone`'s standard bias and rounded to the
  nearest midnight, then given as `YYYY-MM-DDT00:00:00` in that zone.
  Writes need **Windows zone ids** (or UTC): wall times go without an
  offset under a `TimeZoneContext` (+ `StartTimeZone` / `EndTimeZone`);
  IANA names are `unsupported` (WP93's mapping converts them).
- Writes: `CreateItem` with `SendMeetingInvitations`, `UpdateItem`
  replacing every managed field (`SetItemField` / `DeleteItemField`,
  dictionary entries of contacts one by one) with `NeverOverwrite` and
  `SendMeetingInvitationsOrCancellations` — a stale change key is
  `Conflict`; `DeleteItem` `MoveToDeletedItems` with
  `SendMeetingCancellations` and `AffectedTaskOccurrences`. Exceptions and
  cancelled occurrences of a series are applied after the master: the
  occurrence is found through `FindItem` `CalendarView` by UID and
  original date, then updated / deleted. Meeting responses are
  `AcceptItem` / `TentativelyAcceptItem` / `DeclineItem` with the current
  change key; "don't send" saves the response and deletes the draft.
- EWS tasks have no To Do checklist or linked resources (empty);
  regenerating task recurrences are read as none.

### Mock EWS server (`testing`, feature `testing`)

`MockEwsServer` (127.0.0.1, axum, `futures-util` for the streaming body —
both workspace crates) keeps an organization in memory (`fixtures::
organization`): Ada's mailbox (folders, three messages with MIME, a
sub-folder, a weekly series and a single event, a contact, a distribution
list, a task), a shared mailbox, a delegate's mailbox (PIM folders only),
a closed mailbox, a room list with two rooms (they accept invitations and
become busy), directory people, categories, two inbox rules, a delegate.
Items are EWS XML elements, so `SetItemField` / `DeleteItemField` /
indexed fields apply generically; sync states are snapshots (creates,
updates by change key, `ReadFlagChange`s, deletes, paging); occurrences
and exceptions of daily / weekly series; a few Windows zones with EU / US
daylight saving for `TimeZoneContext`. Logins: none, Basic, bearer, and a
scripted Negotiate handshake bound to the TCP connection
(`ScriptedNegotiate` is the client side). Tests mutate the server
(`deliver`, `set_read`, `set_categories`, `move_item`, `add_folder`, …),
inject faults (`ErrorServerBusy` as fault or message, HTTP statuses,
response codes), refuse server versions, close / break / expire streams,
replay recorded Exchange 2019-shaped answers (`fixtures::RECORDED_*`) and
read the request log (operation, version, anchor, zone, auth scheme,
peer, body).

## Consequences

- WP95 builds the engine with `EwsEngine::for_account(account, secrets,
  tokens, negotiate)` (`negotiate` = WP93's `SspiNegotiate` for `auth
  "windows"`), drives `folder_hierarchy` / `folder_items` per mailbox and
  folder, waits with `wait_for_changes` (streaming) and replays writes
  with the new ids moves answer.
- On-premises servers without Focused Inbox: `ExchangeServerInfo::
  focused_inbox` is false and `classify` answers `unsupported`.
- The sent copy is saved by Exchange (`SendAndSaveCopy`); the core must
  not append its own copy to Sent Items for EWS accounts.
