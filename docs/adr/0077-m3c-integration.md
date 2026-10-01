# ADR 0077: M3c integration — Exchange accounts in the core

Status: accepted · 2026-10-02 · M3c WP95

## Context

WP91–WP94 built the Microsoft Graph engine (`nail-proto-graph`, ADR 0073),
the EWS engine (`nail-proto-ews`, ADR 0074), the Exchange common layer
(`nail-exchange`, ADR 0075) and the Exchange UI (ADR 0076) against the M3c
contract (ADR 0072). WP95 wires them into the core, the demo, the UI and
the end-to-end suite.

## Decisions

### The account engine (`nail-core/src/exchange/`)

- `setup.rs` builds the engine of an account: `GraphEngine::for_account`
  (OAuth2 through the core's token manager) or `EwsEngine::for_account`
  with Basic (keychain), OAuth2, or Windows sign-in —
  `nail_exchange::sspi::SspiNegotiate` for the signed-in user or a user
  name with its keychain password; `ServiceOptions::exchange`
  (`ExchangeHooks`) can inject a `NegotiateAuth` (tests). The engine is
  held as `Arc<dyn ExchangeEngine>`; the outbox upcasts it to
  `OutgoingTransport` (trait upcasting), so Exchange accounts send through
  Graph `sendMail` / EWS `CreateItem` and the server files the Sent copy.
- `sync.rs` runs one task per enabled account. A pass: replay the mail
  queue (`ExchangeExecutor`: `RemoteWrite`s by remote id against the
  message's mailbox; a move or trash stores the new id, moves between
  mailboxes or accounts answer `unsupported`) and the Exchange queue
  (categories, automatic replies, server rules, Focused Inbox, one
  operation at a time so a create's id moves later operations), the folder
  hierarchy and the due mail folders of the own mailbox and every shown
  other mailbox, the settings (every 10th pass, after local changes and on
  "Sync now"), then PIM. Graph polls with `PollTimer` (the account's
  `poll-interval` / `max-poll-interval`) and `FolderPoll` (Inbox every
  round, quiet folders less often); EWS with `streaming` waits in
  `wait_for_changes` (60 s at most per wait) and syncs the folders the
  server names; a lost stream or an engine without push falls back to
  polling. A queued write (mail, Exchange or PIM) is replayed at once.
- **Mail model**: messages get a store id derived from their first server
  id (`\0exchange\0<id>`); `messages.remote_id` follows server moves. Folder
  ids derive from (mailbox, remote id), so renames keep them; paths come
  from the parent chain; another mailbox's tree hangs under a root folder
  `@<address>` whose id is stored with the mailbox (`root_folder_id`).
  Folder roles come from well-known names for the own mailbox only (other
  mailboxes' folders are `other`, so the outbox and rules never pick a
  shared Sent / Inbox). An expired cursor lists the folder again and
  merges (messages the server no longer lists go; local-only ones stay).
  Categories map to tags through `mapping::categories`; tags that are not
  valid category names (`$Junk`, …) stay local. Focused Inbox is kept for
  the own Inbox only (`mapping::focus::stored_inference`) and is the
  `focus:` field of search (`MessageFilter::ExchangeFocus`) and of rules
  (`MessageView::exchange_focus`); summaries carry `exchange` from the
  store (`attach_tags`).
- Arrival processing is the same as IMAP / Gmail: new mail of the own
  Inbox / Other folders (not the first listing, not other mailboxes) goes
  to the rule inbox and `NewMail`.

### Settings

- The master category list and the automatic replies replace the stored
  ones (queued local changes win until replayed). A rename on Graph
  replaces the category: `ExchangeSettingsEngine::update_category` now
  answers the new id (contract change), and the store moves the category
  and later queued operations to it.
- **Rules**: the server's inbox rules are read with folder remote ids
  translated to local folder ids (store / UI) and back (engine). With
  `server-rules` on, each Nail rule that applies to the account is
  planned (`mapping::rules::plan`); `server` plans are created or
  replaced when the digest of what was placed changed, and placed rules
  whose Nail rule is gone, changed placement or switched off — all of them
  when `server-rules` is off — are deleted. Rule edits (`queue_sieve`) and
  account changes mark the rules pending and wake the engine. Arrival
  rules skip placed, enabled, error-free rules (`Service::server_rules`).
- `GetVacation` / `UpdateVacation` of an Exchange account map to the
  automatic replies (`mapping::oof`, the local zone); the vacation editor
  lists Exchange accounts.

### PIM (`exchange/pim.rs`)

- One source per account (`exchange:<account>`, kept by
  `reconcile_pim_sources`); collections of the own mailbox and of the
  shown parts of other mailboxes, with `ExchangeCollectionInfo` and their
  Exchange record; `Calendar.account_id` names the account (MCP grants,
  the room finder). Items are mapped by `nail_exchange::mapping` and
  stored with the text `nail-ical` writes; the server id is the object's
  `href`, the change key its `etag` (`apply_server_item` keeps conflicts
  as for DAV). To Do checklist items are subtasks with
  `href = <task>#<checklist id>`; writing a subtask writes its parent; a
  new subtask's local copy is replaced by the server's on the next sync.
- PIM queue replay (`ExchangeReplayer`): creates / updates / deletes
  through `write_pim`; meetings the user organizes go with
  `MeetingSend::ToAll`; an attendee's answer becomes `respond_to_meeting`
  — never iMIP: `scheduling.rs` treats Exchange calendars as
  server-scheduled. EWS writes use UTC wall times (EWS wants Windows zone
  ids; dates stay dates). Creating, renaming or deleting Exchange
  calendars from Nail answers a failed operation.
- Free/busy (`QueryFreeBusy`) asks the running Exchange engines for
  attendees the user's calendars and CalDAV did not answer
  (`FreeBusySource::Exchange`). `FindRooms` caches rooms per room list and
  asks free/busy for them; a `forbidden` answer becomes
  `exchange-warning-rooms-need-consent` (also in the status).

### Account setup

- `DiscoverAccount` gains `trusted_hosts` and `windows_sign_in`; the
  answer gains `exchange: ExchangeDiscoveryInfo` (a prompt
  `confirm-redirect` / `needs-login`, the display name, automapped
  mailboxes). Autodiscover runs next to nail-discovery; its proposals come
  first. It never sends a password: only the signed-in Windows user, on
  request, to hosts of the domain or trusted ones. Microsoft's global
  Autodiscover endpoint (it learns the address) is asked only when the
  domain's MX is Microsoft's; the run is capped at 20 s. Automapped mailboxes
  are remembered per address and stored when the account is added.
- `StartOAuth` gains `protocol`: the flow asks for that protocol's scopes
  (`nail_auth::FlowOptions::scopes`, `OAuthClient::authorize_url_with`),
  so an Exchange account signs in for Graph or EWS only.
- `TestAccount` signs in through the engine (request secret or stored
  one) and reports `exchange:<engine>`, `version:…`, `focused-inbox`.
- The setup dialog mounts the Exchange parts (kind of account, server
  review with the redirect confirmation, Exchange sign-in); the account
  settings the sign-in, options and status.

### UI additions

- `exchange.apply-categories` / dialog `exchange-apply-categories` sets a
  message's categories (tags); reader toolbar and list menu.
- The focused tabs, automatic-replies banner and the two sidebar lists are
  placed in `layout.kdl` (the banner and tabs in the list pane, so the main
  window's root keeps its shape); icon parts of the sidebar lists (and
  `label-list.icon`) got sizes in `theme.kdl`.
- `?mock=1` seeds the Exchange demo (like Gmail; `createMockData` stays
  without it so suites keep their counts).

### demo-multi

- A Graph account `exchange` (`ada@contoso.example`, provider
  `demo-microsoft` on the mock OAuth server) on `MockGraphServer::start_demo`
  and an EWS account `corp` (`ada@corp.example`, Basic over loopback,
  streaming) on `MockEwsServer`, both `enabled=#false` like Gmail;
  `MockAutodiscoverServer` answers `autodiscover.contoso.example` (Exchange
  Online → Graph, rewritten to the mock by `ExchangeHooks::graph_endpoint`)
  and `autodiscover.corp.example` (the mock EWS URL); no SRV, no Microsoft
  lookup. Controls `graph-*` and `ews-*`.
- The mock Graph server's `iCalUId`s were all the same (derived from the
  id's length); they are unique now.

### Tests

- Core: `tests/exchange.rs` (9 scenarios against the mocks), `tests/m3c.rs`
  (served behaviour), unit tests in `exchange/*`. Playwright:
  `e2e/tests/exchange.spec.ts` (6 scenarios covering every WP95 bullet).

## Consequences / deferrals

- Not built: sending as a shared mailbox (Send As / on behalf), copying
  or moving messages between mailboxes or accounts, folder sync policies
  (recent days, headers only) for Exchange folders, creating / renaming
  Exchange calendars, Graph upload sessions (messages over 4 MB), To Do
  linked resources are not written back, subtasks deeper than one level,
  the Focused Inbox switch is per session (UI store), Autodiscover with a
  password or another Windows account (the user enters the server),
  delegate rights (GetDelegate) in the UI.
- Maintainer gates: a real Exchange Online tenant (Graph: nested folders via
  `mailFolders/delta`, `cancelledOccurrences`, `If-Match` change keys,
  MIME creates outside Drafts, `Place.Read.All`) and Exchange Server
  2019 / SE (Kerberos / NTLM through SSPI, streaming, CategoryList,
  ResolveNames), and the EWS retirement date in Exchange Online.
