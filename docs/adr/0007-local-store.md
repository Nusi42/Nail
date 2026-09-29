# ADR 0007: Local store — SQLite schema, blob layout, operation queue, threading

Status: accepted · 2026-09-29 · M0 WP2 (`crates/nail-store`)

## Context

`design.md` ("Storage, sync & search") asks for SQLite metadata in WAL mode,
raw messages in a zstd-compressed content-addressed blob store, and one
operation queue for every user write with offline replay. `plan.md` fixes
`rusqlite` (bundled) with hand-rolled numbered migrations. This ADR records
the choices made while implementing it.

## Decisions

### Libraries

| Crate | Version | Use |
| --- | --- | --- |
| `rusqlite` | 0.40 (`bundled`) | SQLite, compiled in, so every platform gets the same modern SQLite (`unixepoch()`, partial indexes, `ON CONFLICT … DO UPDATE … WHERE`) |
| `zstd` | 0.14 | blob compression (default level 3) |
| `sha2` + `hex` | 0.11 / 0.4 | blob addressing |
| `tempfile` | 3 | atomic blob writes (temp file in the target dir + rename) |
| `tokio` | workspace | `spawn_blocking` for async callers and the replay lock |

### Schema v1

- `schema_version(version, applied_at)` records applied migrations. Migration
  `n` runs in one `IMMEDIATE` transaction with its version row, so a crash
  leaves a whole version. A database newer than the build is refused
  (`Error::SchemaTooNew`), never downgraded. Migrations are append-only.
- `accounts(id, name, email, kind)`. Listing order is insertion order (rowid).
- `folders(id, account_id, parent_id, name, path, role, uidvalidity, uidnext,
  highestmodseq)`, `UNIQUE(account_id, path)`. `unread` / `total` are **not
  stored**; they are computed from `messages` when folders are read, so they
  cannot drift. `HIGHESTMODSEQ` is a 63-bit value in RFC 7162 and fits
  SQLite's signed integer.
- `messages`: every `MessageSummary` field (`from` split into
  `from_name`/`from_email`, `to` as JSON) plus `uid` (unique per folder when
  present), `blob_hash`, `thread_key` and `date_sort` = `unixepoch(date)`
  (0 when unparseable). Lists order by `date_sort DESC, id DESC` so dates in
  different time zones sort by instant and ties are stable.
- `op_queue(id AUTOINCREMENT, account_id, kind, payload JSON, state,
  attempts, last_error, created_at, updated_at)`.
- Foreign keys with `ON DELETE CASCADE` (account → folders, messages, ops;
  folder → messages) and `SET NULL` for a deleted parent folder.
- Pragmas: `journal_mode=WAL`, `synchronous=NORMAL` (durable except for the
  last commits on power loss, which sync repairs), `foreign_keys=ON`,
  `busy_timeout=5s`.

### Blob store

`<data>/blobs/<first 2 hex>/<remaining 62 hex>`, content = zstd of the raw
bytes, name = SHA-256 of the **uncompressed** bytes. `put` of existing content
only hashes (dedup). Writes go to a temp file in the same directory, are
`fsync`ed and renamed without clobbering; a concurrent writer of the same
content is not an error. `get` verifies the hash after decompression and
returns `Error::BlobCorrupt` on mismatch or undecodable data. Blob garbage
collection is the caller's job (`Store::blob_is_referenced`).

### Operation queue

- `Store::enqueue(op)` applies the operation to local tables **and** inserts
  the queue row in one transaction. If the local apply fails nothing is
  queued.
- States `pending → in_flight → done | failed`; a `Retry` outcome returns it
  to `pending` and **ends the pass** so later operations never overtake it.
  After `ReplayPolicy::max_attempts` (default 10) a retry becomes `failed`.
- `Store::open` resets `in_flight` to `pending` (crash recovery), so a
  replayer can see an operation twice and must be idempotent. Operations are
  therefore absolute ("set `\Seen`"), not toggles.
- Payloads are serde JSON (`{"type":"update_flags",…}`) and must stay
  backward compatible once released. M0 has one operation, `UpdateFlags`.
- A failed operation stays applied locally; the next sync restores the
  server's state (server wins for flags, per design).
- `Replayer` is a trait with an `async` method (`impl Future + Send`), so
  sync engines can do network I/O; replay passes on one store are serialized
  by an async mutex.

### Threading

One `rusqlite::Connection` behind `Arc<std::sync::Mutex<_>>`; `Store` is
`Clone + Send + Sync`, with a synchronous API. Async code calls
`Store::call(|s| …)`, which runs the closure on tokio's blocking pool.

Alternatives considered: a connection pool (`r2d2`/`deadpool-sqlite`) buys
concurrent readers, but SQLite serializes writers anyway, M0 has one UI and
one sync engine, and a single connection makes our own transactions free of
`SQLITE_BUSY`. `tokio-rusqlite` (a dedicated thread per connection) is
equivalent in effect but adds a dependency and an actor layer. A read-only
pool can be added behind the same API if profiling shows contention.

## Consequences

- Sync code writes via the direct CRUD methods (`upsert_*`, `update_flags`,
  `delete_messages_by_uid`, `clear_folder` on `UIDVALIDITY` change); user
  actions must use `enqueue`.
- A sync that upserts a message while a flag operation for it is still
  pending overwrites the local flags with the server's until the replay
  lands. M1 (flag write-back) should skip or re-apply pending operations when
  merging server state.
