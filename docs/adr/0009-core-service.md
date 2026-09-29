# ADR 0009: Core service, sync engine, server, CLI and desktop shell

Status: accepted · 2026-09-29 · M0 WP5

## Context

WP1–WP4 delivered the parts: configuration (`nail-config`), the store
(`nail-store`), MIME parsing and the IMAP client (`nail-mime`,
`nail-proto-imap`), test servers and a corpus (`nail-testkit`) and the React
renderer. WP5 wires them into one core that every transport uses: the
browser through `nail serve`, the desktop window through Tauri, and tests.

## Decisions

### One `Service`, many transports

`nail_core::Service` (cheap to clone, `Arc` inside) owns the `Store`, the
`BlobStore`, the `ConfigManager` and the sync engine. It has one entry point,
`handle(Request) -> Response`, which never fails (errors become
`Response::Error`), and one event stream, `subscribe()` (a tokio broadcast
channel, 256 events deep; a subscriber that falls behind loses events and
logs a warning).

- `Service::open(ServiceOptions)` opens `<data>/nail.sqlite3` + `<data>/blobs/`,
  loads `<config>/theme.kdl` and `layout.kdl` (missing or invalid files fall
  back to the defaults) and starts the config watcher. Valid changes are
  broadcast as `ConfigChanged`, invalid ones as `ConfigError`.
- **Start-up config errors.** An error found before any client was
  connected would otherwise be lost. `Service::config_error_events()` returns
  the current errors. The WebSocket server sends them to each client right
  after it connects, and the desktop shell emits them after answering
  `GetConfig`.
- Store work runs on the blocking pool (`Store::call`, `spawn_blocking` for
  parsing), never on the async executor.
- `ListMessages` caps `limit` at 1000 (`MAX_PAGE_SIZE`).
- `GetMessage` parses the raw message from the blob store on every call.
  Parsing a message takes about a millisecond, so no body cache exists yet.

### Deterministic ids

Ids are opaque to clients. The core derives them from the server identity:
`f-<16 hex>` = SHA-256 of (account, folder path) and `m-<16 hex>` = SHA-256
of (account, folder path, UIDVALIDITY, UID), with the parts length-prefixed.
Re-syncing or re-seeding the same mailbox therefore updates rows instead of
duplicating them, and ids survive restarts.

### Read-only sync engine (M0 scope)

One pass per account: connect, `LIST`, mirror the folder tree (new folders
are added; local folders missing on the server are deleted with their
messages and their unreferenced blobs), then `plan_fetch` per selectable
folder from the stored UIDVALIDITY/UIDNEXT. A changed UIDVALIDITY clears the
folder and fetches it again. The plan is fetched batch by batch (batches
sized by `RFC822.SIZE`); each batch is stored and the folder's UIDNEXT is
checkpointed right after it, so memory stays bounded and an interrupted pass
resumes after the last stored batch. Raw messages go to the blob store.
Summaries come from `nail-mime`.

A folder that fails cleanly (`NO`, `BAD`, a withheld message) is skipped
and listed in `SyncReport::failed_folders`; the other folders are still
synced and the pass ends with one `SyncFailed` naming the failed folders.
Connection-level errors (lost connection, timeout, TLS) and store errors end
the pass.
The thread key is the root of `References`, else `In-Reply-To`, else the
message's own `Message-ID`; JWZ threading comes in M1.

Events: `SyncProgress` counts one unit for the folder list plus one per
selectable folder (`done == total` means finished), `MailChanged` names the
folders that got messages or were added or removed, and a failed pass
broadcasts **`SyncFailed {account_id, code, message}`**. This event is new in
the protocol. Without it, a wrong password or an unreachable server would be
invisible to the user. The UI shows it in the status bar until the next pass
makes progress.

Only one pass per account runs at a time; a second one fails with
`unavailable`. `SyncNow` starts passes in the background and answers
`SyncStarted` at once. For an account without server settings (the local
demo account) it reports a finished pass (`SyncProgress 1/1`).

**Not in M0:** flag changes and expunges of already-synced messages, IDLE,
periodic polling, CONDSTORE/QRESYNC. The UI shows the state of the last
pass. These come with full IMAP in M1.

### No account configuration yet

The design keeps account settings in a separate `accounts` config and keeps
credentials in the OS keychain. Both come with account setup and autoconfig
in M1. In M0, IMAP accounts are registered in code
(`Service::add_imap_account(Account, ImapAccount)`), and the credentials are
kept only in memory. `nail serve --seed demo-imap` uses this to sync the
demo corpus from an in-process mock IMAP server. This exercises the whole
sync path with no network access.

### Demo data

`nail_core::demo` (feature `demo`, on by default) has two parts:

- `seed_demo(store, blobs)` loads the `nail-testkit` corpus as a local
  account `demo`. It is idempotent and needs no network.
- `start_demo_imap()` starts the mock IMAP server and returns the account
  and settings for it.

The plan put `seed_demo` into `nail-testkit`. It lives in `nail-core`
instead, because seeding needs the core's ingest path (blob store, MIME
parsing, id derivation). A copy in the testkit would drift from the real
thing. `nail-core` therefore depends on `nail-testkit` at runtime, behind the
`demo` feature. The corpus is embedded at build time, so shipped binaries
can seed demo data.

### `nail-server`

The server uses axum 0.8 and tower-http 0.7 and has three routes:

- `/api/ws`: the protocol of ADR 0002. Every request runs in its own task,
  so a slow `GetMessage` does not block a `Ping`. A malformed frame whose
  `id` can be read gets an `invalid_request` response; other malformed
  frames are dropped. Frames are limited to 1 MiB.
- `/api/blob/{hash}`: the raw message as `message/rfc822`, sent as an
  attachment with `CSP: default-src 'none'; sandbox`. An invalid hash gets
  400 and an unknown hash gets 404.
- Everything else serves `ui/dist` with an SPA fallback to `index.html`.
  Unknown `/api/*` paths return 404.

M0 security:

- The server binds to loopback addresses only; `Server::bind` refuses
  anything else.
- Every route accepts only a `Host` of `127.0.0.1:<port>`,
  `localhost:<port>` or `[::1]:<port>` (the bound port), and an `Origin`, if
  present, of `http://` plus one of those. Anything else gets 403. Checking
  `Origin` against the request's own `Host` is not enough: under DNS
  rebinding an attacker's page at `http://evil.example:<port>` resolves to
  127.0.0.1 and sends matching `Host` and `Origin` headers. The allowlist
  also blocks cross-site WebSocket hijacking. Clients that send no `Origin`
  (tools, tests) are accepted.
- Every response carries a strict CSP (`script-src 'self'`,
  `frame-ancestors 'none'`, `object-src 'none'`), `nosniff`,
  `Referrer-Policy: no-referrer` and COOP `same-origin`. `style-src` allows
  `'unsafe-inline'`, because the theme compiler injects one `<style>`
  element. This matches the desktop shell's CSP.
- There is no authentication yet. TLS, pairing, passkeys and sessions come
  with the remote UI in M2.

### `nail` CLI

The CLI has two commands:

- `nail serve [--port 4780] [--data-dir] [--config-dir] [--ui-dir] [--seed demo|demo-imap] [--locale] [--open] [--no-watch] [-v]`
- `nail config schema [theme|layout]`

Directory defaults:

- **Data:** `%LOCALAPPDATA%\Nail`
- **Config:** `%APPDATA%\Nail`

These are the design's installer locations (`nail_core::paths`, from the
`dirs` crate).

The UI directory is taken from the first of these that is set or exists:

1. `--ui-dir`
2. `$NAIL_UI_DIR`
3. `ui/` next to the executable
4. the repository's `ui/dist` (development builds)

`nail serve` writes exactly one line to stdout: the URL. Logs go to stderr
(`NAIL_LOG` sets the filter). Scripts and the Playwright harness read the
port from that line when `--port 0` is used.

**Deviation: CLI text is not localized.** Clap help texts, errors and log
lines are English developer output. They are not UI strings in Fluent
catalogs. A Rust Fluent loader for user-facing CLI output can be added once
the CLI has user-facing features (M1: `nail mcp`).

### Desktop shell (`app/src-tauri`)

The shell hosts the same `Service` in-process. It has one command,
`nail_call(request) -> Response`, which always resolves: errors are
`Response::Error`, never a rejected promise. A background task forwards
every broadcast event to the webview on `nail://event`. Command-line
options (`--data-dir`, `--config-dir`, `--seed demo`, `--locale`) exist for
development and testing; unknown arguments fall back to the defaults.

Debug builds load the UI from the Vite dev server (`devUrl`), and
`npm run tauri dev` starts that server. To run the built `ui/dist` without
the dev server, add `--features tauri/custom-protocol`. The Tauri CLI does
this for `tauri build`.

## Consequences

- Adding a request means adding one `match` arm in `Service::dispatch`.
  Every transport picks it up without changes.
- Tests drive the service directly (`crates/nail-core/tests`), over a real
  WebSocket (`crates/nail-server/tests`) and through the binary
  (`crates/nail-cli/tests`). The sync engine is tested against the mock IMAP
  server.
- Without account setup, M0 users cannot add their own IMAP account. The
  M0 gate only needs the demo data.
