# ADR 0032 (M2a WP33): Data protection — encryption at rest, sender authentication, link safety

Status: accepted · 2026-09-29 · M2a WP33

## Context

Plan section 7, WP33: encrypt the database, the blob store and the search
index at rest with the key in the OS keychain, migrate crash-safely between
plain and encrypted with progress, decide what decrypted end-to-end
content may be indexed, parse `Authentication-Results`, and warn about
dangerous links and attachments. Constraints: no OpenSSL build dependency,
no GPL-family licence, Windows MSVC first, encryption at rest ships **off**
until the maintainer decides. The contract (ADR 0029) fixed the protocol types
(`VaultStatus`, `SecuritySettings`, `AuthResults`, `MessageWarning`) and the
store v5 `vault_state` row; this package may not add migrations.

## Decision 1: the database — evaluated options

| Option | Crypto | Windows MSVC | Licence | Verdict |
| --- | --- | --- | --- | --- |
| SQLCipher via `rusqlite` `bundled-sqlcipher` | OpenSSL `libcrypto` (Windows: `OPENSSL_DIR` or `bundled-sqlcipher-vendored-openssl`, which builds OpenSSL from source with Perl/NASM) | only with OpenSSL | BSD-3 | **Rejected**: OpenSSL build dependency (checked in `libsqlite3-sys` 0.38.2 `build.rs`: on Windows the only non-OpenSSL provider, CommonCrypto, is macOS-only). |
| SQLCipher with libtomcrypt / NSS | libtomcrypt (public domain) | would build | BSD-3 | **Rejected for now**: `libsqlite3-sys` has no switch for it; needs a fork of the sys crate. |
| SQLite3MultipleCiphers (sqlite3mc) | own C implementations (AES-256 SQLCipher-compatible, ChaCha20-Poly1305, …) | builds with MSVC | MIT | **Preferred for full-file encryption, not feasible in this package**: `libsqlite3-sys` cannot compile it, and a second sys crate conflicts on `links = "sqlite3"`. It needs `[patch.crates-io]` of `libsqlite3-sys` with a vendored fork that swaps the amalgamation — a root `Cargo.toml` decision with its own review. Recorded as a follow-up. |
| Encrypting SQLite VFS in Rust | XChaCha20-Poly1305 | yes | ours | **Rejected**: raw `sqlite3_vfs` FFI (much `unsafe`), page / WAL / journal handling, reserved bytes — too much risk for the gain over the next option. |
| **Column-level sealing** of sensitive values | XChaCha20-Poly1305 (pure Rust) | yes | ours | **Chosen.** |

**What is sealed** (`nail_store::SEALED_COLUMNS`): `messages.preview` (the
first ~200 characters of every body) and `drafts.payload` (whole drafts:
body, recipients, attachment list). The store registers two SQL functions
on its connection, `nail_seal(column, value)` and `nail_open(column,
value)` (`SQLITE_DIRECTONLY`, so a trigger or view stored in the file
cannot call them). Writes go through `nail_seal`, reads through
`nail_open`; a sealed value is TEXT `"\u{1}nlv1:" + base64(sealed)`, so no
read path changes type. `nail_seal` leaves sealed values alone and
`nail_open` passes plain ones through, which makes conversion idempotent.

**What stays plain** (residual risk, documented for users): subjects,
addresses, dates, flags, folder names, thread keys (normalized subjects),
message-ids, collected addresses, the operation queue, rule runs, MCP
audit entries and the keyring's public parts. They are SQL predicates,
sort keys or join keys; sealing them would break filtering, threading and
sorting. Secret keys in the keyring were already encrypted (ADR 0029).
The blob hash (SHA-256 of the raw message) stays plain; it allows
confirming that a *known* message is present, nothing more.

After a conversion the store runs `PRAGMA wal_checkpoint(TRUNCATE)` and
`VACUUM` with `temp_store=MEMORY` (so no plaintext copy lands in `%TEMP%`),
and `secure_delete` is on whenever a cipher is installed.

## Decision 2: one sealed-data format (`nail-vault::format`)

Chunked XChaCha20-Poly1305: a 24-byte header (magic `NLV\x01`, algorithm
1, chunk-size exponent, a random 16-byte nonce prefix), then chunks of
32 KiB plaintext + 16-byte tag. Chunk `i` uses nonce `prefix ‖ i`
(64-bit counter); the associated data is the header, the context (what
the data is: `nail/blob/<hash>`, `nail/index/<file>`,
`nail/field/<table.column>`, `nail/vault/key-check`) and a final-chunk
flag. So data cannot be moved to another place, chunks cannot be
reordered, and truncation at a chunk boundary is detected. The plaintext
length follows from the file length, so any range can be read by
decrypting only the chunks it touches. XChaCha's 192-bit nonce lets the
prefix be random without a nonce registry.

## Decision 3: blobs and the search index

- **Blobs**: `BlobStore` seals the zstd-compressed bytes (context = the
  hash) when a cipher is installed in `Seal` mode; reads accept both forms
  (zstd frames start with `28 B5 2F FD`, sealed data with `NLV\x01`).
  `BlobStore::convert` rewrites one file via temp file + fsync + rename
  after verifying the hash. File names stay the hash.
- **Search index**: `nail_vault::SealedDirectory`, a tantivy `Directory`
  wrapping `MmapDirectory`: streaming sealed writes (`open_write`, final
  chunk on `terminate`), random-access reads with an 8-chunk cache per
  file, sealed `atomic_write` / `atomic_read` (`meta.json`,
  `.managed.json`), locks and watches delegated. Plain files are refused
  once encrypted (a planted plain segment is not read) and accepted only
  during a migration. `nail_search::SearchIndex::open_with` takes the
  wrapper (`DirectoryWrapper`); an index that cannot be read through it is
  rebuilt, since it is derived data. The version file stays plain.

## Decision 4: the key

A random 256-bit data key, stored hex-encoded in the OS keychain through
`nail_api::engine::SecretStore` (Windows Credential Manager via
`nail-auth`; tests use `MemorySecretStore`) under a **random** reference
`keychain:nail/vault-<16 hex>/data-key` kept in `vault_state.key_ref`, so
profiles and test runs never share a key. `vault_state.key_check` holds a
sealed known plaintext: a missing key is `locked`, a wrong key `crypto`.
The key is zeroed on drop (`zeroize`) and never logged (`Debug` prints
`DataKey(..)`). **If the keychain entry is lost, encrypted data is lost**
(the index can be rebuilt, blobs re-synced from IMAP; local-only data such
as drafts and POP3 mail cannot). A recovery-key export is a follow-up.

## Decision 5: crash-safe migration (`nail_vault::Vault`)

State machine in the `vault_state` row: `plain → encrypting → encrypted →
decrypting → plain`, phases `database → blobs → index`, progress
`done/total`, last error (`"<code>|<message>"`). Rules:

- Keychain first, row second when creating the key; row first, keychain
  second when deleting it. An orphaned key is harmless; a row naming a
  missing key would lose data.
- Every unit is atomic and self-describing: a database batch is one
  SQLite transaction (a failure mid-batch rolls it back — tested); a
  file is replaced by rename; sealed values / files carry a prefix /
  magic. So a crash anywhere just repeats some work; the next
  `Vault::migrate` resumes from the recorded phase.
- `encrypting`: new writes are sealed, reads accept both; `decrypting`:
  new writes plain, reads both. The app keeps working during a
  migration; only the index phase needs the search index closed
  (`Vault::migrate_until(Some(Index))` runs the first two phases with it
  open).
- `set_enabled` during a migration reverses it; progress callbacks get a
  `VaultStatus` after every unit and may return `ControlFlow::Break` to
  pause.
- Temporary blob files are removed only when older than 10 minutes (a
  running write's temp file is young).

Tests: full encrypt → restart → decrypt round trip with a byte scan of
every file for plaintext; a simulated crash (panic) at every third
progress step in both directions, plus a second crash on the resumed run,
covering all three phases; pause and reverse mid-way; torn temp files;
missing / wrong key; a failing keychain; a failure inside a database
batch.

## Decision 6: decrypted-content indexing policy (`nail_search::IndexPolicy`)

Off by default. `IndexPolicy::document(outer, decrypted)` indexes an
encrypted message by its outer addresses and outer subject only (not the
ciphertext body, not `encrypted.asc`). For accounts in
`SecuritySettings::index_decrypted` it indexes the decrypted subject
(header protection), body and attachment names and marks the document
(`decrypted_in` field, search schema v2 → existing indexes are rebuilt
once). The index then counts as sensitive (`index_is_sensitive`), which
the UI shows; with encryption at rest on it is encrypted like everything
else. `withdrawn()` / `granted()` tell the core which accounts to purge
(`SearchIndex::remove_decrypted`) or reindex. Decrypted content never
enters the database: `message_security` caches status only.

## Decision 7: `Authentication-Results` (`nail_mime::auth_results`)

RFC 8601 parser (comments, quoted strings, spaces around `=` `.` `/`,
method versions, `reason`, address values, `none`, version check) plus
`ARC-Authentication-Results` (`i=`). Only a trusted `authserv-id` counts,
and only the receiving server's own block: the topmost trusted header and
trusted headers directly below it up to the next `Received` header. A
forged header with the trusted id below the server's `Received` line is
ignored. DKIM: all results; SPF / DMARC / ARC: first; ARC falls back to
the newest trusted `ARC-Authentication-Results`. `aligned` uses relaxed
alignment with an approximated organizational domain (no Public Suffix
List; private suffixes like `github.io` count as organizations —
informational only, the server's DMARC verdict decides).
`learn_authserv_id` proposes the id that is on top in most recent mail
(*changed by ADR 0035:* bound to the receiving server's `Received` host,
and only a suggestion the user saves).

## Decision 8: link and attachment safety (`nail_mime::links`)

`message_warnings(&MessageBody)` numbers links like the UI (the `<a href>`
of the sanitized HTML, else text URLs) and reports at most one warning per
link: look-alike (IDN with mixed scripts, or whose UTS #39 skeleton is
ASCII, with the imitated domain), suspicious (IP host, credentials in the
URL, `file:` / `smb:`), mismatch (the text is a URL or domain of another
organizational domain; plain text like "Click here" is not compared).
Attachments: bidi control characters, double extensions and padding
(`invoice.pdf.exe`), executables and scripts, macro formats, disk images,
shortcuts, and declared types contradicting the name. The demo corpus
raises no warning (tested).

## Consequences

- WP35 wires it (see the integrator requests in the WP33 report): the
  vault in `Service` (open at start, resume a pending migration, events
  `VaultProgress`), `SetVaultEncryption`, the index wrapper, the policy in
  ingest, `auth_results` and `message_warnings` in `GetMessageBody`, a
  trusted `authserv-id` per account, `Locked` / `Crypto` store errors in
  `CoreError`.
- Encryption at rest is **off** by default (`nail_vault::DEFAULT_STATE`).
- Full-file database encryption via sqlite3mc stays an option for later
  (root `Cargo.toml` patch of `libsqlite3-sys`, own ADR).
- Not protected: data in memory, the Windows page file / hibernation file,
  earlier plaintext on SSD blocks (a migration rewrites files, it does not
  erase old blocks). The user docs recommend BitLocker in addition.
