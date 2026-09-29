# ADR 0002: One transport-agnostic RPC protocol, types generated with ts-rs

Status: accepted · 2026-09-29 · M0 WP0

## Context

The same React bundle runs in the Tauri desktop window and in a browser
connected to `nail serve`, and later MCP and plug-ins use the same API
(`design.md`, Architecture and Remote UI). The UI must therefore never depend
on Tauri, and the types on both sides must not drift.

## Decision

### Shape

All messages are defined once in the `nail-api` crate (`crates/nail-api/src/protocol.rs`):

- **`Request`** — an internally tagged enum, `{"type": "<Variant>", ...fields}`.
  M0: `Ping`, `GetConfig`, `ListAccounts`, `ListFolders {account_id}`,
  `ListMessages {folder_id, offset, limit}`, `GetMessage {id}`,
  `SyncNow {account_id | null}`.
- **`Response`** — exactly one per request, same tagging: `Pong {version, protocol}`,
  `Config {theme, layout, locale}`, `Accounts`, `Folders`,
  `Messages {folder_id, offset, total, messages}`, `Message`, `SyncStarted`,
  and `Error {code, message}`, which may answer any request.
- **`Event`** — pushed by the core: `ConfigChanged {theme, layout}`,
  `ConfigError {file, line, column, message}`, `MailChanged {folder_ids}`,
  `SyncProgress {account_id, done, total}`.
- **Errors** — `ErrorCode` is a closed, snake_case enum (`not_found`,
  `auth_failed`, …). `message` is an English diagnostic for logs; the UI shows
  the Fluent message `error-<code>` (underscores → hyphens), so no
  user-visible text originates in Rust.

### Transports

| Transport | Client → core | Core → client |
| --- | --- | --- |
| Tauri (desktop) | `invoke("nail_call", { request })` → `Response` | events on `nail://event`, payload `Event` |
| WebSocket `/api/ws` (`nail serve`) | `RpcRequest {id, request}` | `ServerMessage`: `{"kind":"response","id","response"}` or `{"kind":"event","event"}` |

`id` is chosen by the client and stays below 2^53. Tauri needs no envelope
because `invoke` already correlates calls.

### Conventions

- JSON field names are snake_case (as in Rust); protocol variant tags are
  PascalCase; data enums (roles, kinds, layout node types) are lowercase /
  kebab-case to match the KDL files.
- `Option` fields are always present and `null` when empty; the TypeScript
  types say `T | null`, never `T?`.
- Ids are opaque strings. 64-bit integers are exported as `number`.
- `PROTOCOL_VERSION` (returned by `Pong`) is bumped on incompatible changes.

### Type generation: ts-rs

`nail_api::export_typescript(dir)` exports every type reachable from the
protocol roots with [`ts-rs`](https://crates.io/crates/ts-rs) 12, one file per
type plus an `index.ts` barrel, into `ui/src/api/generated/`. The files are
checked in. The `bindings` integration test regenerates into a temp dir and
fails when the checked-in files differ; `npm run gen:api` updates them.
We call the export explicitly instead of using `#[ts(export)]`, which would
write files into the crate directory on every `cargo test`.

## Alternatives considered

- **tauri-specta** (named in `design.md`): generates typed Tauri command
  bindings, but ties the generated client to Tauri IPC. We need the same types
  over a WebSocket and, later, MCP; a single `nail_call` command with a
  tagged `Request` makes Tauri just one transport. `specta` alone would work
  too; ts-rs is the more widely used, serde-aware option with no runtime part.
- **One Tauri command per operation**: more boilerplate on both transports
  and a second place to keep in sync.
- **JSON-RPC 2.0**: adds method strings and positional/named params on top of
  what the tagged enums already express; no client library benefit here.

## Consequences

- Adding an operation = one `Request` variant, one `Response` variant, a
  handler in `nail-core`, and `npm run gen:api`.
- Both transports share the exact JSON, so the UI's `MockTransport` and the
  server tests can use the same fixtures.
