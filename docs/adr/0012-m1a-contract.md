# ADR 0012: M1a contract — protocol additions, engine traits, crate shapes

Status: accepted · 2026-09-29 · M1a WP10

## Context

Wave M1a (plan section 6) turns Nail into a daily-driver mail client: full
IMAP, POP3, SMTP, OAuth2, discovery, threading, search, virtual accounts, a
plain-text composer and an outbox. Four packages build in parallel against
one contract (WP11 IMAP, WP12 outgoing/POP3/auth/discovery, WP13 store v2 /
threading / search, WP14-UI), then WP14-core wires everything. This ADR fixes
that contract. The accounts file format is ADR 0013, the new libraries
ADR 0014.

## Decisions

### Protocol additions (`crates/nail-api`)

Conventions of ADR 0002 hold: tagged enums (`type` for protocol variants),
snake_case fields, lowercase / kebab-case data enums, `Option` always
present as `null`, opaque string ids. `PROTOCOL_VERSION` stays 1: every
change is an addition, and `MessageSummary`'s new fields have serde
defaults.

New modules: `accounts` (configuration, identities, virtual accounts, setup,
status), `ops` (writes and queue status), `search`, `compose` (drafts,
outbox), and `engine` (Rust-only traits, below).

New ids: `IdentityId`, `ThreadId`, `DraftId`, `OutboxId`, `OpId`,
`OAuthFlowId`. `AccountId::ALL` (`"all"`) is the built-in "All accounts"
virtual account.

| Request | Response | Notes |
| --- | --- | --- |
| `DiscoverAccount {email}` | `Discovered {email, candidates}` | ranked `DiscoveredAccount`s (source, incoming, outgoing); empty = enter manually |
| `TestAccount {config, secrets}` | `AccountTested {result}` | per-server `EndpointTestResult {error, message, capabilities}`; a failed login is a result, not an `Error` |
| `StartOAuth {provider, login_hint}` | `OAuthStarted {flow_id, authorize_url, opened}` | result arrives as `Event::OAuthFinished`; tokens stay in the core, referenced later by `SecretValue::OAuth2 {flow_id}` |
| `CancelOAuth {flow_id}` | `Done` | |
| `GetAccountsConfig` | `AccountsConfig {config}` | the whole `accounts.kdl` model |
| `AddAccount {config, secrets}` | `AccountSaved {config}` | empty `config.id` = derive one; `conflict` if taken |
| `UpdateAccount {config, secrets}` | `AccountSaved {config}` | secrets: changed ones only |
| `RemoveAccount {account_id}` | `Done` | also leaves virtual accounts, deletes keychain entries and local mail |
| `ListIdentities {account_id?}` | `Identities {identities}` | real, virtual (members') or all |
| `ListVirtualAccounts` / `AddVirtualAccount {config}` / `UpdateVirtualAccount {config}` / `RemoveVirtualAccount {account_id}` | `VirtualAccounts` / `VirtualAccountSaved` / `Done` | |
| `ListAccountStatuses` | `AccountStatuses {statuses}` | changes: `Event::AccountStatus` |
| `UpdateFlags {message_ids, change}` | `Queued {op_ids}` | `FlagChange`: seen/flagged/answered/draft (`null` = keep) + add/remove tags |
| `MoveMessages {message_ids, target}` | `Queued {op_ids}` | `MoveTarget`: `{"folder_id"}` or `{"role"}` (resolved per source account) |
| `DeleteMessages {message_ids, permanent}` | `Queued {op_ids}` | not permanent = to Trash; already in Trash = expunge |
| `ListThreads {folder_id, offset, limit}` | `Threads {folder_id, offset, total, threads}` | `ThreadSummary` |
| `GetThread {thread_id}` | `Thread {thread, messages}` | across folders and (virtual) accounts, oldest first |
| `Search {query}` | `SearchResults {offset, total, hits}` or `InvalidQuery {error}` | `SearchError {kind, message, start, end}`, UTF-16 offsets |
| `CheckQuery {text}` | `QueryChecked {error}` | live feedback for the search bar |
| `SaveDraft {draft}` / `GetDraft {draft_id}` / `DeleteDraft {draft_id}` | `DraftSaved {draft_id}` / `Draft {draft}` / `Done` | plain text in M1a; `html`, `markdown`, `attachments` reserved (a draft using them is `unsupported` until M1b) |
| `SendDraft {draft_id, delay_secs, send_at}` | `SendQueued {entry}` | `delay_secs: null` = `accounts.kdl` `undo-send-delay` (default 10 s) |
| `CancelSend {outbox_id}` | `SendCancelled {draft_id}` | `conflict` once sending started |
| `ListOutbox` | `Outbox {entries}` | |

New events: `OpStatus {status}` (queue replay progress), `SendStatus
{outbox_id, state, error, message}`, `NewMail {account_id, folder_id,
message_ids}` (notifications), `AccountStatus {status}` (online /
connecting / offline / auth-error / disabled), `AccountsChanged` (refetch
account data; an invalid `accounts.kdl` is a `ConfigError` with
`file: "accounts.kdl"`), `OAuthFinished {flow_id, error, message}`.

`MessageSummary` gains `thread_id`, `tags` and `duplicates`
(`MessageLocation`s of the same Message-ID in other members of a virtual
account). Until WP13 fills them they are `null` / empty.

New error codes: `conflict`, `tls`, `too_large`, `rejected` (permanent
server refusal, e.g. SMTP 5xx), `cancelled`. Each has an
`error-<code>` string in `locales/en/app.ftl`. The UI shows query errors
with `search-error-<kind>` and account states with `account-state-<state>`;
a test checks all three sets of keys exist.

Large payloads in `Request` / `Response` variants are boxed
(`clippy::large_enum_variant`); serde serializes a `Box<T>` exactly as `T`.

Until WP14 wires them, `nail-core` answers every new request with
`unsupported` (explicit match arms, so a later variant cannot be forgotten)
and so does the UI's `MockTransport`.

### Accounts as listed to clients

`ListAccounts` returns the built-in "All accounts" virtual account first
(id `all`, kind `virtual`, **empty `name`**: the UI shows its Fluent label
`account-all`), then the real accounts and the user's virtual accounts in
file order. Virtual accounts have folders like real ones (`ListFolders`
with the virtual account's id); their folder ids are owned by the core like
all ids. The `AccountConfig` behind a real account is fetched with
`GetAccountsConfig`.

### Engine traits (`nail_api::engine`)

The seams several crates share live in `nail-api` (every crate already
depends on it), so that `nail-proto-smtp`, `nail-auth` and `nail-core` need
no dependencies on each other. They are Rust-only (not exported to
TypeScript) and object-safe (async methods return `BoxFuture`):

- `SecretStore` (blocking `get` / `set` / `delete` by `SecretRef`; call
  from `spawn_blocking`). `nail-auth` implements it on the OS keychain;
  `MemorySecretStore` (in `nail-api`) serves tests, demos and the mock.
- `OutgoingTransport::send(&Envelope, &[u8]) -> SendReceipt`: submit raw
  MIME. `nail-proto-smtp` implements it (later: Graph, Gmail API).
- `AccessTokenProvider::access_token(provider, &SecretRef, force_refresh)`:
  `nail-auth` implements it; the core calls it before an XOAUTH2 /
  OAUTHBEARER login and hands the token to the protocol client.
- `EngineError {code, message, retryable}`: the one error type across those
  seams. `retryable` decides between "keep queued" and "fail".

Replaying queued writes keeps using `nail_store::Replayer` (ADR 0007); the
core implements one replayer per protocol (IMAP: STORE / MOVE / COPY+DELETE /
APPEND; POP3: delete policies). No new trait is needed for that.

### Expected shape of the new crates

These crates are stubs after WP10 (each compiles and has one test). Their
owners decide details, but the integrator (WP14-core) expects roughly:

- **`nail-proto-smtp`** (WP12): `SmtpConfig {host, port, tls: TlsMode,
  trust (platform | custom DER roots), timeout}`, `SmtpCredentials {None |
  Password {username, password} | OAuth2 {username, access_token}}`, a
  transport implementing `OutgoingTransport`, and `test(config,
  credentials) -> Result<EndpointTestResult-like info, EngineError>`
  listing EHLO capabilities.
- **`nail-proto-pop3`** (WP12): `Pop3Config` like `ImapConfig`, a client
  with `connect`, `capa`, `uidl`, `top`, `retr`, `dele`, `quit`; and a pure
  function planning server deletions from `Pop3Options`, the download
  times and the locally deleted UIDs.
- **`nail-auth`** (WP12): `KeychainSecretStore` (`SecretStore`), a provider
  registry built from `AccountsConfig::oauth_providers` (Google and
  Microsoft endpoints built in), an OAuth2 flow (PKCE, loopback redirect on
  `127.0.0.1:0`) returning an authorize URL and a future of tokens, and a
  token manager implementing `AccessTokenProvider` that stores refresh
  tokens through `SecretStore`. `nail_auth::http_client()` builds the shared
  `reqwest` client.
- **`nail-discovery`** (WP12): `discover(email, &Discoverer) ->
  Vec<DiscoveredAccount>` where `Discoverer` holds injectable HTTP and SRV
  lookups (traits) plus the ISPDB base URL, so tests use
  `nail-testkit-net` and never the network.
- **`nail-search`** (WP13): `SearchIndex` (open on disk or in RAM, add /
  remove / commit), `parse_query(&str) -> Result<Query, SearchError>` with a
  public AST, and `search(&Query, scope, offset, limit)` returning message
  ids with scores.
- **`nail-testkit-net`** (WP12): mock SMTP, POP3, OAuth2, HTTP (autoconfig /
  ISPDB / Autodiscover) servers and DNS answers, all on `127.0.0.1:0`,
  recording what they receive. The mock IMAP server stays in
  `nail-testkit` (WP11 extends it).

### Where configuration and state live

- `accounts.kdl` (ADR 0013) is the single source of truth for accounts,
  identities, virtual accounts and OAuth2 client ids. The store keeps the
  `accounts` rows it needs for foreign keys and listing, updated by the
  core's account manager; it does **not** need its own copy of identities
  or virtual-account definitions (a deviation from the plan's WP13 bullet,
  which listed them for store v2): two copies would have to be kept in
  sync, and the account manager can pass members and identities into
  queries. WP13 may still add tables if a query needs a join.
- Drafts and the outbox are runtime state and belong in store v2 (WP13),
  next to the operation queue.
- Op ids on the wire are the store's `OpId` formatted by the core (e.g.
  `op-42`); clients treat them as opaque.

## Consequences

- WP11–WP14 can build against fixed types; the TypeScript bindings in
  `ui/src/api/generated/` already contain everything WP14-UI needs.
- Adding the M1b composer fields later is additive.
- Rust-only traits in `nail-api` mean the contract crate is no longer
  purely data; it stays free of I/O and runtime dependencies (only
  `std::future`).
