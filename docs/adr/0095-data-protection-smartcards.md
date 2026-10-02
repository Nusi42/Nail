# ADR 0095: full-file database encryption with SQLite3 Multiple Ciphers, the vault file and recovery key, PKCS#11 smartcards

Status: accepted · 2026-10-02 · M4c WP122

## Context

Plan section 9, WP122: complete data protection (full-file database
encryption via SQLite3MultipleCiphers as a `[patch]` of `libsqlite3-sys`
if it builds on Windows MSVC, else a documented fallback), the vault
recovery key (export, verify, restore, rotate) and PKCS#11 smartcards for
OpenPGP and S/MIME with PIN prompts. The contract is ADR 0094. ADR 0032
(M2a) had kept the database a plain SQLite file with sealed columns
because SQLCipher needs OpenSSL on MSVC and SQLite3MultipleCiphers was not
reachable through `rusqlite` without patching `libsqlite3-sys`.

Constraints: no GPL family, no OpenSSL / aws-lc, nothing written into the
repository by builds, no network except package registries, crash safety
for every migration, PINs never stored.

## Decisions

### 1. SQLite3 Multiple Ciphers through a `[patch]` of `libsqlite3-sys` — feasible, adopted

**Evaluation.**

| Option | Result |
| --- | --- |
| `libsqlite3-sys` `bundled-sqlcipher(-vendored-openssl)` | needs OpenSSL (banned) |
| A crate wrapping sqlite3mc for `rusqlite` | none on crates.io usable with `rusqlite` 0.40 (`walletkit-sqlite` is an app-specific wrapper, not a `libsqlite3-sys`) |
| Vendoring the amalgamation into the repo | 13 MB of C in OneDrive; downloading it from GitHub is outside the allowed network |
| **`sqlite3mc-src` 205.1.0 (crates.io) + a local `libsqlite3-sys` 0.38.2 stand-in** | builds on Windows MSVC in about 80 s (debug), all store / vault / core tests pass |

`sqlite3mc-src` ships the untouched SQLite3 Multiple Ciphers 2.5.1
amalgamation (SQLite 3.53.4) checked against the release's signed
checksums and compiles nothing itself. Licence expression: `MIT AND
CC0-1.0 AND Unlicense AND (CC0-1.0 OR Apache-2.0)`. The Unlicense part is
miniz, compiled only with `SQLITE_ENABLE_COMPRESS` / `SQLAR` / `ZIPFILE`,
which Nail does not set; it is a public-domain dedication, not copyleft.
`deny.toml` does not list `Unlicense`, so the integrator adds a
per-crate exception (see "Follow-ups").

**The stand-in** lives in `crates/nail-store/libsqlite3-sys-sqlite3mc/`
(package `libsqlite3-sys`, version 0.38.2, MIT, `publish = false`; not a
workspace member). The root `Cargo.toml` patches crates.io with it:

```toml
[patch.crates-io]
libsqlite3-sys = { path = "crates/nail-store/libsqlite3-sys-sqlite3mc" }
```

It keeps the original crate's Rust API (`src/lib.rs`, `src/error.rs`,
MIT, the rusqlite developers) and the pre-generated bindings of SQLite
3.53.2 (`bindings/bindgen_bundled_version.rs`; 3.53.4 is a patch release
with the same API), and its `build.rs` compiles the amalgamation with the
original bundled build's defines plus `SQLITE_TEMP_STORE=2` (temporary
tables in memory, so no plaintext temp file next to an encrypted
database). All original features exist so `rusqlite`'s feature list
resolves; SQLCipher, build-time bindgen, loadable extensions, session /
pre-update hooks and gecko builds fail with `compile_error!`. No OpenSSL,
no bindgen / libclang; the lock loses `pkg-config` / `vcpkg` users and
gains `sqlite3mc-src`. Plain databases (and every other `rusqlite` user,
e.g. `nail-portability` reading Thunderbird's `abook.sqlite`) behave as
before.

Upgrading: bump `sqlite3mc-src` (its version encodes SQLite3MC) together
with `rusqlite`; when the `libsqlite3-sys` version changes, copy that
version's `src/` and bindings into the stand-in and keep its `build.rs`.

### 2. Cipher, key and file migration

- Cipher: SQLite3MC's default **`chacha20`** scheme (sqleet:
  ChaCha20-Poly1305 per page, authenticated, a per-file KDF salt in the
  first 16 bytes); the WAL is encrypted too. Keys are applied with
  `PRAGMA cipher = 'chacha20'; PRAGMA hexkey = '…'` as the first
  statements of a connection (never in a URI that could be logged); the
  SQL text is zeroed after use.
- The **database key** is derived from the vault's data key:
  `SHA-256("nail/vault/database-file/v1" ‖ 0x00 ‖ data key)`
  (`nail_vault::key::database_key`), so the keychain keeps one secret.
- **Which form a file is in** is read from its header
  (`nail_store::file_encryption`: `SQLite format 3\0` = plain, anything
  else = encrypted, empty / missing = missing). Nothing else has to be
  recorded first, which is what makes the conversion crash-safe.
- `Store::open_with_key(path, key)`: an encrypted file without a key is
  `locked`, with a wrong key `crypto`; a plain file opens plain even with
  a key; a new file is created encrypted. `Store::open` = no key.
- **Conversion** `Store::rewrite_file(Some(key) | None)`, used for plain →
  encrypted, encrypted → plain and re-keying, under the store's connection
  lock: checkpoint the WAL (`TRUNCATE`), `VACUUM INTO
  'file:<db>.rewrite?cipher=chacha20&hexkey=…'` (or `?key=` for a plain
  copy: attached files would otherwise inherit the main key), `fsync` the
  copy, open it with the target key and run `quick_check`, close the old
  connection (its WAL is empty), remove `-wal` / `-shm`, rename the copy
  over the database (retried briefly: scanners and sync clients hold files
  on Windows), reopen with the per-connection settings (sealing functions,
  `secure_delete` while a cipher is installed). Crash before the rename:
  the old file is intact and the stray `.rewrite` file is removed at the
  next open or rewrite. Crash after it: the new file is complete and its
  header says what it is. A failed rename reopens the old file with the
  old key. The rewrite also compacts, so no free page or WAL frame of the
  previous form survives in the new file (the old file's blocks on an SSD
  are outside Nail's reach, as documented since M2a).

### 3. Full-file encryption joins the vault migration; column sealing stays

Encrypting keeps the M2a work (sealed columns, sealed blobs, sealed
index) and **adds** the database file: the database phase now ends with
`rewrite_file(Some(database_key))` instead of `compact()`; decrypting ends
it with `rewrite_file(None)`. No new `VaultPhase` (that would be a
contract change); the step is idempotent because it checks the header.
Keeping column sealing costs little and keeps sealed values sealed inside
backups and exports made from the store.

`Vault::new` keeps full-file encryption **off** (M2a behaviour, so the
current core, which opens the store with `Store::open`, keeps working
until WP125 rewires start-up); `Vault::with_full_file_encryption(true)` and
`nail_vault::open_data_dir` turn it on. Decrypting always turns an
encrypted file plain. A vault encrypted before M4c (column sealing only)
gets its file encrypted by `Vault::encrypt_database_file` (a crash-safe
rewrite; a no-op otherwise), which WP125 runs in the background after
start.

### 4. The vault file (`vault.json`) — where the recovery-wrapped key lives

With an encrypted database the `vault_state` row cannot be read before
the key is known, so what opening and recovery need lives in
`<data dir>/vault.json` (`nail_vault::vaultfile`): format version, the
keychain reference of the data key, the key check value (hex), whether
the database was last written encrypted (informational) and the recovery
record (the data key sealed with the recovery wrapping key, creation and
last verification time). None of it is secret. Written atomically
(temporary file, `fsync`, rename). Order of writes: keychain → vault
file → row when a key is created; row → vault file → keychain when it is
forgotten. `Vault::open` re-syncs the file from the row (the row wins
while readable; a recovery record of another key is dropped), which also
upgrades M2a data directories. In-memory stores keep it in memory.

`nail_vault::open_data_dir(dir, index_dir, secrets)` is what the core
calls at start instead of `nail_store::open_data_dir` + `VaultCtl::open`:
it returns `OpenedDataDir::Ready { store, blobs, vault, open_error }` or,
when the database file is encrypted and its key is missing or wrong,
`OpenedDataDir::Locked(LockedVault)` — no store, only `status()`
(`state: encrypted`, `needs_recovery` while the key is not in the
keychain, the recovery status, the error with its Fluent id),
`verify_recovery_key` and `restore_key`; afterwards the caller opens the
directory again.

### 5. Recovery key

- 32 bytes from the OS generator, shown once in the contract's 56-character
  form (`nail_api::recovery`), never stored.
- Wrapping key = `SHA-256("nail/vault/recovery-key/v1" ‖ 0x00 ‖ recovery
  key)` (a 256-bit random input needs no slow KDF); the data key is sealed
  with Nail's sealed format (XChaCha20-Poly1305, associated data
  `nail/vault/recovery-wrapped-data-key`).
- `Vault::create_recovery_key` (encrypted or encrypting vault; replaces =
  **rotates** an older one), `verify_recovery_key` (`valid` + records
  `verified_at`; a mistyped key is `invalid_request` with
  `recovery-key-issue-*`), `restore_key` (puts the data key back under the
  vault file's keychain reference after checking it against the key check
  value; `auth_failed` + `recovery-key-wrong` for a wrong key),
  `remove_recovery_key`. Turning encryption off forgets the recovery key.
- `VaultStatus.needs_recovery` = encrypted data and no data key in the
  keychain; `VaultStatus.recovery` from the vault file.
- The recovery kit file (`CreateRecoveryKey.save_to`) is written by the
  core with the `recovery-kit-*` texts of `core-vault.ftl`.

### 6. Smartcards (`nail-pkcs11`)

- `Module` trait (tokens, objects, login, logout, sign, decrypt, ECDH;
  a missing login is `Ok(None)`, not an error). `CryptokiModule` over the
  vendor DLL with `cryptoki` 0.12.1: one read-only session per slot,
  reopened once when the token dropped it; objects found by class and
  `CKA_ID`; ECDH derives an extractable session secret, reads and
  destroys it. Error classes map to `SmartcardError`.
- `Smartcards` loads the enabled modules of `SmartcardSettings` (load
  failures become `SmartcardModuleProblem`s), lists tokens as the
  contract's `SmartcardToken` (algorithm from the key, its public key
  object or its certificate; certificate facts with SHA-256; usages;
  `key_id` / `openpgp_fingerprint` left to the caller) and resolves
  `SmartcardKeyRef`s to `CardKey`s (token found by serial, so a card may
  move to another reader).
- **PIN flow** (`CardKey::sign` / `decrypt` / `ecdh`): try the operation;
  when the card wants a login, ask the `PinPrompter` (`PinRequest` with
  purpose, key label, `attempts_left = 1` on the final try, `retry` after
  a wrong PIN), log in, retry. A locked PIN stops without a prompt, a
  cancelled prompt is `pin-cancelled`, at most five prompts per
  operation. **PINs are never stored**: the session stays logged in for
  `pin_cache_minutes` (0 = log out after every operation) and
  `expire_logins` / `logout_all` end it. `BlockingPinSource` adapts the
  core's async `PinSource` (`Event::PinRequested` / `AnswerPinPrompt`) by
  waiting on the calling thread (`nail_pkcs11::block_on`); card operations
  therefore run on blocking threads (`spawn_blocking`), never on an async
  task.
- Not supported: keys with `CKA_ALWAYS_AUTHENTICATE` (context-specific
  login per operation; PIV "digital signature" slot), EdDSA / Curve25519
  OpenPGP card keys (no portable PKCS#11 mechanism for OpenPGP's legacy
  formats), protected authentication paths (PIN pads) beyond what the
  module does by itself.
- **Tests without hardware**: `nail-pkcs11`'s `testing` feature provides
  `SoftToken`, an in-memory module with real RSA / P-256 keys that behaves
  like a card where Nail cares (login needed, three PIN tries then locked,
  `pin_count_low` / `pin_final_try`, removal, moving to another slot,
  unsupported mechanisms). SoftHSM is not installed on the build machines
  and would be a native test dependency; the cryptoki path itself is
  covered by load-failure tests and the shared `Module` contract.

### 7. S/MIME and OpenPGP on the card

- **S/MIME** (`nail_smime::smartcard`): `SmartcardKey` implements
  `PrivateKeyOps` — RSA PKCS#1 v1.5 (DigestInfo built by Nail), RSA-PSS
  (salt = digest length), ECDSA (raw `r‖s` → DER), RSA key transport
  (PKCS#1 v1.5, OAEP) and ECDH (P-256 / P-384) on the card.
  `Keyring::with_smartcards(&dyn KeyResolver)`;
  `Keyring::use_smartcard_key(&SmartcardKeyRef)` takes the certificate
  stored on the card (or a keyring certificate with the same public key),
  checks it matches the card key and stores the own key with
  `SecretStorage::Smartcard`, `KeySource::Smartcard`, `secret_ref` = the
  JSON of the reference; `CryptoKey.smartcard` is filled.
- **OpenPGP** (`nail_pgp::smartcard`): rPGP allows external signers
  through `SigningKey`; `CardSigningKey` pairs the keyring's public
  (sub)key packet with the card key (RSA and ECDSA P-256 / P-384).
  Decryption bypasses rPGP's key types: the RSA PKESK is decrypted on the
  card (ciphertext left-padded to the modulus), the session key checked
  (algorithm byte for v3, two-byte checksum) and the message opened with
  `decrypt_with_session_key`. A card failure while reading (PIN cancelled,
  card missing) shows as `locked` with the key id; signing failures
  surface as `Error::Smartcard` with the card's `smartcard-error-*` text.
  The OpenPGP public key must already be in the keyring
  (`Keyring::use_smartcard_key` matches primary key or subkey by public
  key); an OpenPGP card's separate signing and decryption keys are
  attached one after the other, so `secret_ref` may hold a **JSON array**
  of references (`CryptoKey.smartcard` shows the first).
  `UnlockedKey.secret` became `Option<SignedSecretKey>`
  (`UnlockedKey::secret_key()`; Autocrypt setup messages cannot export a
  card key: `unsupported`). `Keyring::fingerprint_for_card_key` gives
  `SmartcardObject.openpgp_fingerprint`.

## Consequences

- Subjects, addresses and every other column are encrypted on disk once
  WP125 opens data directories with `nail_vault::open_data_dir` and the
  user turns encryption at rest on; user docs must change their "what is
  encrypted" table then.
- A missing data key now means *no store at all* for a full-file
  encrypted directory: the core must run a locked mode (vault status,
  recovery requests) instead of a degraded store.
- Builds compile SQLite3MC instead of SQLite (about the same time).
- New dependencies: `sqlite3mc-src` (build), `zeroize` in `nail-store`;
  `serde` / `serde_json` / `chrono` / `sha2` in `nail-vault`; `rsa`,
  `chrono`, `const-oid` (+ `p256`, `rand`, `sha1`, `sha2-legacy` for
  `testing`) in `nail-pkcs11`; `nail-pkcs11` + `serde_json` in
  `nail-smime`; `rsa`, `p256`, `hex`, `serde_json` in `nail-pgp` — all
  already in the lock, MIT / Apache-2.0.

## Follow-ups (integrator, WP125)

- `deny.toml`: `[[licenses.exceptions]] crate = "sqlite3mc-src", allow =
  ["Unlicense"]` (or add `Unlicense` to the allow list).
- Core start-up through `nail_vault::open_data_dir`; locked mode; the
  recovery and smartcard requests; `SmartcardsChanged` polling; card
  operations on blocking threads with `BlockingPinSource`.
- User docs: `data-protection.md` table update (see the WP122 report).
