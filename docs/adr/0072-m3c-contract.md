# ADR 0072: M3c contract — Exchange accounts (Graph, EWS), Windows sign-in, shared mailboxes, categories, Focused Inbox, inbox rules, automatic replies, rooms, To Do, store v12, slots

Status: accepted · 2026-10-01 · M3c WP90

## Context

Wave M3c (plan section 8) adds Microsoft Exchange: the Microsoft Graph
engine for Exchange Online (WP91, `nail-proto-graph`), the EWS engine for
Exchange Server on-premises (WP92, `nail-proto-ews`), the common layer —
Autodiscover with engine choice, Windows sign-in through SSPI, the mapping
between Exchange and Nail models (WP93, `nail-exchange`) — and the Exchange
UI (WP94), built in parallel and integrated by WP95. This ADR fixes the
contract they build against, the store schema they share and the slots
that keep them apart.

Constraints (plan "Scope decisions for M3", design "Microsoft Exchange"):
no GPL / LGPL / AGPL dependency, no OpenSSL / aws-lc; no non-loopback
sockets (ADR 0057; every mock binds 127.0.0.1, DNS through the OS
resolver); Graph push needs a public webhook, so Graph accounts poll delta
queries with adaptive intervals while EWS uses streaming notifications;
on-premises sign-in uses Windows SSPI through the `windows` crate (no NTLM
of our own), with Basic over TLS and OAuth2 for hybrid as options;
messages are fetched as full MIME (Graph `$value`, EWS `MimeContent`);
server-side inbox rules and automatic replies replace Sieve for Exchange;
shared mailboxes and delegate access are extra folder trees under the
account; Microsoft is retiring EWS in Exchange Online (announced from
2026-10-01, from memory — the maintainer re-checks the timeline), so Exchange
Online uses Graph; strings only from Fluent catalogs; the core owns data and
every write goes through a queue.

## Decisions

### The account kind (`nail_api::exchange`, `accounts.kdl`)

- `IncomingServer::Exchange {server, options: ExchangeOptions}` →
  `AccountKind::Exchange`; `sends_itself()` (no `smtp`;
  `config-exchange-no-smtp`). `ExchangeOptions`: `engine` (`graph` default
  / `ews`), `url` (EWS endpoint from Autodiscover; default
  `https://<host>/EWS/Exchange.asmx`), `poll-interval` (≥ 10, default 30) /
  `max-poll-interval` (default 300), `streaming` (EWS, default on),
  `server-rules` (default on). `MailProtocol` gains `graph` and `ews` (port
  443 / 80; `IncomingServer::protocol()` follows the engine).
- `accounts.kdl`: `exchange engine=… host=… url=… … { auth … }`. Graph's
  host defaults to `graph.microsoft.com`; EWS needs `host` or `url`
  (`config-exchange-needs-host`; the host is taken from `url`).
  `ExchangeOptions::issues` holds the rules the reader enforces
  (`config-exchange-*`): Graph needs `oauth2`, EWS takes `windows`,
  `password` (Basic over TLS) or `oauth2`; `tls="plaintext"` only for
  loopback hosts; no `starttls`; `url` / `streaming` only with EWS;
  `url` https (or loopback http). `ExchangeOptions::warnings`:
  `ews-retired-online` for EWS against Exchange Online.
- **Windows sign-in**: `AuthConfig::Windows {username?, secret?,
  package?}` (`auth "windows" username="CORP\ada" package="ntlm"`).
  Without `username` the signed-in Windows user signs in and no secret is
  stored (`secret_kind()` is `None`); with one, the password is a keychain
  reference like any password. `SspiPackage` `negotiate` (default) /
  `kerberos` / `ntlm`. Only Exchange accounts accept it
  (`config-auth-windows-needs-exchange`); every other login path answers
  `unsupported`.
- **IMAP fallback**: an ordinary IMAP + SMTP account; Autodiscover (WP93)
  proposes it next to the Exchange engines when the server lists IMAP.
- OAuth: `nail_auth::mail_scopes(microsoft, [Graph])` = `GRAPH_SCOPES`
  (mail incl. shared, send, mailbox settings, calendars, contacts, To Do,
  `Place.Read.All` — admin consent in many tenants, else rooms answer
  `forbidden` with `exchange-warning-rooms-need-consent` — and
  `offline_access`); `[Ews]` = `EWS.AccessAsUser.All`. Microsoft issues
  tokens per resource, so an Exchange account signs in for its engine's
  protocol only (WP95 passes the protocol to the OAuth flow).

### Mail mapping

- Messages are stored once with the server id in `messages.remote_id`
  (kept across local moves; Exchange ids change on server moves, which the
  engine reports as the write's new id). Exchange extras are
  `ExchangeMessageInfo {item_id, conversation_id, inference, mailbox}` on
  `MessageSummary.exchange` (optional, like `gmail`).
- **Focused Inbox** = `InferenceClassification` (`focused` / `other`), a
  filterable property: query `focus:focused` / `focus:other` (WP95 adds it
  to `nail-search` and the rules' message view, like `category:`).
  `ClassifyFocused {message_ids, classification, always}` moves messages
  (and with `always` adds a sender override) through the Exchange
  operation queue.
- **Categories ↔ tags**: a category is the Nail tag of the same name
  (case-insensitive like Outlook); the master list
  (`ExchangeCategory`, `CategoryColor` `none` / `preset0`…`preset24` with
  `rgb()` and EWS colour indexes) gives tags their colours. A tag set on an
  Exchange message is written as a category (Exchange accepts categories
  outside the master list), so tags never create master entries. Renaming
  a category renames the tag on messages through the **mail** queue (the
  core, WP95); deleting one leaves the tags (as Outlook does).
- **Folders**: well-known folders (`ExchangeWellKnownFolder`, Graph names /
  EWS distinguished ids) give roles; the store keys Exchange folders by
  remote id. Folder paths are display paths; another mailbox's tree lives
  under `@<address>` (`mailbox_folder_path`, `split_mailbox_folder_path`).

### Shared mailboxes and delegate access

`ExchangeMailboxSettings {address, name?, kind: shared | delegate, mail,
calendar, contacts, tasks}` (issues `exchange-mailbox-issue-*`),
`ExchangeMailbox` adds origin (`user` / `automapped` from Autodiscover's
`AlternativeMailbox`), state (`pending`, `synced`, `access-denied`,
`not-found`, `error`), its root folder and its calendars / address books.
Mailboxes are account **data** in the store, not `accounts.kdl` (the server
automaps them; removing an automapped one hides it). Requests:
`ListExchangeMailboxes`, `AddExchangeMailbox`, `UpdateExchangeMailbox`,
`RemoveExchangeMailbox`. An identity whose address is a mailbox's sends as
that mailbox (Send As / on behalf is the server's permission; WP95).

### Server-side rules and automatic replies

- `exchange::rules`: `ExchangeInboxRule` (id, name, sequence, enabled,
  conditions / exceptions `ExchangeRuleConditions`, actions
  `ExchangeRuleActions` — the subset of Graph `messageRule` that EWS
  shares —, `has_unsupported` (read-only apart from on / off), `in_error`,
  `nail_rule_id`). Nail rules are **placed** as inbox rules where
  expressible (`sieve::RulePlacement` reused; reasons
  `exchange-rule-reason-*` from WP93's mapping); the user's own server
  rules are listed and can be switched on / off or deleted.
  `ExchangeRulesStatus` (state `disabled` / `pending` / `syncing` /
  `in-sync` / `failed`). Requests `GetExchangeRules`, `SyncExchangeRules`,
  `SetExchangeInboxRuleEnabled`, `DeleteExchangeInboxRule`.
- `exchange::oof`: `AutomaticRepliesSettings {status: disabled |
  always-enabled | scheduled, start?, end? (RFC 3339 UTC), internal_reply,
  external_reply, external_audience: none | contacts-only | all}` (issues
  `exchange-oof-issue-*`). Replies are **plain text** in the contract; the
  engines write simple HTML and read HTML back as text (Outlook formatting
  is lost once edited in Nail). `GetAutomaticReplies` /
  `SetAutomaticReplies` (queued). The generic vacation UI maps through
  WP93's `VacationSettings` ↔ automatic replies.

### Rooms, free/busy, To Do

- `RoomList`, `Room` (address, name, capacity, building, floor,
  accessible, features, room list), `RoomAvailability` (free?, busy
  periods, error). `ListRoomLists {refresh}`, `FindRooms {room_list?,
  from, to, min_capacity?, refresh}` (cached in the store; availability
  through the account's free/busy). `QueryFreeBusy` answers rooms and
  people of an Exchange account with `FreeBusySource::Exchange`.
- **PIM**: one source per Exchange account,
  `PimSourceKind::Exchange`, id `exchange:<account>`
  (`exchange_pim_source_id`; `:` never appears in configured names), not
  in `accounts.kdl`. Calendars / address books carry
  `ExchangeCollectionInfo {mailbox?, role?, todo}` on `Calendar.exchange` /
  `AddressBook.exchange` (roles `default-calendar`, `default-contacts`,
  `default-tasks`, `flagged-emails`). **Microsoft To Do** lists (Graph) are
  task lists (`todo: true`); EWS uses the Tasks folder.

### Status and events

`GetExchangeStatus` → `ExchangeStatus` (engine, server version, push
`streaming` / `polling` / `none`, last sync, throttled until, folders
synced / total, pending Exchange ops, warnings). One event
`ExchangeChanged {account_id, areas: [status | mailboxes | categories |
automatic-replies | rules | rooms]}`. 18 requests in all; every one
answers `invalid_request` for accounts of other kinds (WP95).

### Engine seams (`nail_api::exchange::engine`, Rust only)

Gmail's single-cursor `MailApiEngine` does not fit Exchange (a cursor per
folder, a hierarchy cursor, several mailboxes, PIM and settings on the same
connection), so both engines implement `ExchangeEngine` =
`ExchangeMailEngine` (server info, `folder_hierarchy`, `folder_items`
per folder → the existing `DeltaOutcome` / `SyncDelta` with
`CursorExpired`, `fetch_mime`, `write` of the existing `RemoteWrite`s —
tags are categories, a move answers the new id —, `classify`,
`wait_for_changes` = EWS streaming or `None` for polling) +
`ExchangeSettingsEngine` (master categories, automatic replies, inbox
rules) + `ExchangeDirectoryEngine` (room lists, rooms, `check_mailbox`) +
`ExchangePimEngine` (`pim_folders`, `pim_changes` per folder with cursors,
`write_pim` with `MeetingSend`, `respond_to_meeting`) +
`FreeBusyProvider` + `OutgoingTransport`. `mailbox` arguments name another
mailbox (`None` = own). `RemoteMessage` / `RemoteMessageChange` gain
`change_key` and `exchange`.

PIM items cross the seam in **provider-neutral Exchange shapes**
(`nail_api::exchange::pim`: `ExchangeEvent` with Windows / IANA zone
wall times, recurrence patterns and ranges, attendee kinds and responses,
exceptions and cancelled occurrences; `ExchangeTask` with To Do checklist
items and linked resources; `ExchangeContact`; `ExchangePimFolder`). The
engines convert their wire formats to these; WP93's mapping converts them
to Nail's PIM model (zones → IANA via `nail_ical::tz`, patterns ↔ `RRULE`,
exceptions ↔ overrides, checklists ↔ subtasks). Protocol code and model
mapping stay in different crates.

Windows sign-in crosses as `NegotiateAuth` / `NegotiateSession` /
`NegotiateStep` (Negotiate tokens per round; one session per HTTP
connection because NTLM binds to it) and `http_spn`; `nail-exchange`
implements it with SSPI, `nail-proto-ews` only consumes it (tests use a
scripted fake), so the EWS engine has no platform code.

### Remote access

`nail-remote-auth::required_access`: status, mailboxes / categories /
rules / automatic-reply reads and rooms are `read`; categories and
`ClassifyFocused` are `organize`; mailbox changes, automatic replies and
rule changes are `admin` (like `UpdateVacation`, `SyncSieve`,
`UpdateAccount`). The core (`check_remote_request`) needs the account in
the grant (`ClassifyFocused`: its messages); `ExchangeChanged` goes to
grants of the account.

### Served now

Nothing: the core routes every M3c request to `nail-core/src/exchange.rs`,
which answers `unsupported` until WP95; an Exchange account's engine
reports itself offline with `unsupported`; `TestAccount` answers
`unsupported` for its incoming side; sending from an Exchange account
answers `unsupported`; `Windows` sign-in elsewhere answers `unsupported`.
`MessageSummary.exchange` stays empty; `Calendar.exchange` is filled from
the store. `tests/m3c.rs` checks all 18 requests, the account kind and
status; `tests/remote.rs` the remote rules. The UI mock
(`features/exchange/mock.ts`) answers the same.

### Store schema v12 (`nail-store`)

- `pim_sources` is **rebuilt** (SQLite cannot change a `CHECK`): `kind`
  takes `exchange`, `account_id` ties the source to its account (cascade),
  ids up to 128 characters. The migration runner gained
  `Migration::foreign_keys_off` (`Migration::rebuild`): foreign keys are
  switched off around that step's transaction (a `DROP TABLE` would
  otherwise cascade) and `pragma_foreign_key_check` must be empty before
  the commit; they are restored even when the step fails (tested).
- `exchange_sync` (engine, server version, EWS subscription + watermark,
  throttling, rules state, last sync), `exchange_mailbox_sync` (hierarchy
  cursor per mailbox), `exchange_folders` (remote id, change key, parent,
  well-known name, items cursor, interrupted first sync — per folder),
  `exchange_items` (mailbox, change key, conversation, Focused Inbox),
  `exchange_pim_collections` (remote id, change key, role, To Do, cursor —
  per collection), `exchange_pim_items` (remote id, change key per PIM
  object), `exchange_mailboxes`, `exchange_categories`,
  `exchange_automatic_replies` (**sealed**), `exchange_rules` (server
  rules + the Nail rule / digest placed), `exchange_rooms` (cache),
  `exchange_ops` (the Exchange operation queue; payload **sealed**).
- Store APIs are part of the contract (`src/exchange.rs`, tested): sync
  state and cursors, folders / items, the PIM source, collections and
  items, mailboxes (automapping, hiding, state), categories (server list
  keeps pending ones; counts by tag), automatic replies (a pending local
  change wins over server reads), rules (placement kept across re-reads),
  rooms, and the queue (`enqueue_exchange_op` applies locally in the same
  transaction; `finish_exchange_op` moves a created category from its
  `local-…` id to the server's, marks things synced, rolls back failed
  creates). Parallel packages add no migrations; WP95 appends v13 if
  needed.

### Crates and libraries

No new third-party crate. `nail-proto-graph`: reqwest (TLS from
`nail-auth`, ring), serde_json, base64, chrono, axum for the mock
(`testing`). `nail-proto-ews`: the same plus quick-xml. `nail-exchange`:
quick-xml, serde_json, nail-ical / chrono-tz, nail-discovery (SRV through
the OS resolver), axum for the mock Autodiscover (`testing`) and, on
Windows, the workspace `windows` crate with
`Win32_Security_Authentication_Identity` and `Win32_Security_Credentials`
(module-level `allow(unsafe_code)` like `nail-smime`). Smoke tests: each
mock answers over loopback through the nail-auth client; SOAP envelopes and
`MimeContent`; SSPI reports Negotiate / Kerberos / NTLM on Windows (no
network, no credentials); Autodiscover POX via quick-xml; Windows zone
names through nail-ical. `cargo deny` / `cargo tree -i openssl-sys` /
`aws-lc-rs` unchanged.

### Collision-free slots

| Area | Slot | Owner |
| --- | --- | --- |
| Graph engine, mock Graph server (feature `testing`, fixtures) | `crates/nail-proto-graph/`, `locales/{en,de}/core-graph.ftl` | WP91 |
| EWS engine, mock EWS SOAP server (feature `testing`, fixtures) | `crates/nail-proto-ews/`, `locales/{en,de}/core-ews.ftl` | WP92 |
| Autodiscover, SSPI `NegotiateAuth`, mapping layer, mock Autodiscover | `crates/nail-exchange/`, `locales/{en,de}/core-exchange.ftl` (contract ids pre-filled) | WP93 |
| Exchange UI | `ui/src/features/exchange/` (registered, mock routed, state `data.features.exchange`, messages in the coverage test), `crates/nail-config/src/widgets/exchange.rs` (future widgets `focused-tabs`, `automatic-replies-banner`), `locales/{en,de}/exchange.ftl` | WP94 |

Integrator-owned during M3c (report, do not edit): `Cargo.toml` (ask
before adding dependencies), `Cargo.lock` beyond a package's own new
dependency, `nail-api`, `nail-store`, `nail-core`, `nail-server`,
`nail-mcp`, `nail-search`, `nail-rules`, `nail-remote-auth`, `nail-auth`,
`nail-discovery`, `nail-config` (except the widget file), `app/src-tauri`,
`defaults/`, `e2e/`, `ui/src/widgets/`, other features' directories,
`docs/adr` numbering. Contract or schema changes go to WP95 as
`apiRequests`.

## Consequences

- WP91–WP94 compile against fixed types and generated TypeScript; the
  suites stay green in between.
- WP95 wires: the engines into the account manager (sync loop per folder
  with `AdaptivePoll` or `wait_for_changes`, hierarchy per mailbox,
  replay with remote ids, the Exchange operation queue, sending,
  `TestAccount`), the Exchange PIM source (collections, PIM queue replay,
  meeting responses, To Do), free/busy and rooms, categories ↔ tags and
  `focus:` in search and rules, rule placement and automatic replies (incl.
  the vacation mapping), mailboxes (Autodiscover automapping, folder trees),
  account setup through Autodiscover with engine choice, OAuth scopes per
  engine, MCP coverage, demo-multi with mock Graph / EWS servers, and the
  M3c Playwright scenarios.
- Exhaustive matches (core dispatch, `check_remote_request`,
  `remote_event`, `required_access`) fail to compile when a request or
  event is added; `EXCHANGE_REQUESTS` (18) in the UI mock and
  `tests/m3c.rs` (18) count them.
