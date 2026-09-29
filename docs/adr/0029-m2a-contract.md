# ADR 0029: M2a contract — security protocol, store v5, crypto crate stubs, slots

Status: accepted · 2026-09-29 · M2a WP30

## Context

Wave M2a (plan section 7) adds end-to-end encryption and data protection:
OpenPGP and Autocrypt (WP31), S/MIME (WP32), data protection — encryption
at rest, `Authentication-Results`, link safety (WP33) — and the security UI
(WP34), built by four agents in parallel and integrated by WP35. This ADR
fixes the contract they build against, the store schema they share and the
slots that keep them out of each other's files.

Constraints from the plan's scope decisions: Nail is closed source for now,
so no GPL / LGPL / AGPL dependency (OpenPGP uses rPGP, not Sequoia); no
OpenSSL build dependency unless an ADR shows there is no alternative;
encryption at rest ships **off** until the maintainer decides.

## Decisions

### Protocol additions (`crates/nail-api`, module `security`)

Conventions of ADR 0002 / 0012 hold. `PROTOCOL_VERSION` stays 1: every
change to an existing type is an **optional** field (absent on the wire =
the old behaviour, `#[ts(optional)]`), so M1 clients and stored JSON keep
working. New id: `KeyId` (`key-<n>`, opaque; fingerprints are separate
fields). New error codes: `locked` (a passphrase is needed), `no_key` (a
needed key is missing or unusable), `crypto` (signing / encryption /
verification failed, malformed key data); each has an `error-*` message.

**Read side.** `MessageBody` gains `security?: SecurityStatus`,
`auth_results?: AuthResults` and `warnings?: MessageWarning[]`.

- `SecurityStatus {technology, signatures: SignatureInfo[], encryption?,
  header_protection: none | protected | mismatch, partial, inline_pgp}`.
  `SignatureInfo {state: valid | invalid | unknown-key | key-expired |
  key-revoked | unsupported | error, signer, signer_matches_from, key_id,
  fingerprint, trust, signed_at, chain?}`. `EncryptionInfo {state:
  decrypted | no-key | locked | failed | unsupported, key_id, recipients,
  algorithm}`. S/MIME chains: `CertificateChain {state: valid |
  untrusted-root | incomplete | expired | wrong-usage | invalid,
  revocation: not-checked | good | revoked, certificates}` — revocation is
  always `not-checked` in M2.
- `TrustLevel` (ordered worst → best): `rejected < unknown <
  autocrypt-gossip < autocrypt < verified < own`. A stored user decision
  (`TrustDecision`: `verified`, `rejected`, `unset`) wins over the key's
  source.
- `AuthResults {authserv_id, dkim[], spf?, dmarc?, arc?}` with
  `AuthMethodResult {verdict, domain, selector, aligned, reason}` and
  `AuthVerdict` (RFC 8601 values + `policy` + `unknown`; `as_rule_result()`
  maps to the rules engine's `AuthResult`). Only headers from a trusted
  `authserv-id` (the account's own receiving server) may produce a result;
  `summary()` is the badge verdict (DMARC, else best DKIM, else SPF).
- `MessageWarning` (tag `kind`): `link-mismatch`, `lookalike-domain`,
  `suspicious-link` — links numbered zero-based in document order among the
  `<a href>` of `sanitized_html` (or the URLs of the text body) — and
  `dangerous-attachment {index, filename, risk: executable | macro |
  disguised-name | container | shortcut | type-mismatch}`.

For decrypted mail, `text`, `sanitized_html` and `attachments` carry the
decrypted content; the core never persists it unless decrypted indexing is
enabled for the account.

**Keys and certificates** — one keyring for both technologies
(`CryptoKey {id, technology, fingerprint, user_ids, emails, algorithm,
created_at, expires_at, expired, revoked, can_sign, can_encrypt,
has_secret, secret_storage: keychain | passphrase | windows-store, source,
trust, issuer?, serial?, chain?}`):

| Request | Response | Package |
| --- | --- | --- |
| `ListKeys {filter: {technology?, text?, own_only}}` | `Keys {keys}` (own first) | WP31/32 → WP35 |
| `GetKey {key_id}` | `Key {key}` | WP35 |
| `ImportKeys {data, encoding: text \| base64}` (armored / binary OpenPGP, PEM / DER certificates; merges) | `KeysImported {result: {keys, created, updated, issues}}` | WP31/32 |
| `ImportPkcs12 {data, passphrase, protection}` | `KeysImported`; `auth_failed` for a wrong passphrase | WP32 |
| `ExportPublicKey {key_id, encoding}` | `KeyExported {filename, mime, encoding, data}` | WP31/32 |
| `GenerateKey {identity_id, algorithm: curve25519 \| rsa4096, expires_days?, protection}` | `KeyGenerated {key}` | WP31 |
| `DeleteKey {key_id, delete_secret}` | `Done`; `conflict` for an own key without `delete_secret` | WP35 |
| `SetKeyTrust {key_id, decision}` | `KeySaved {key}` | WP35 |
| `UnlockKey {key_id, passphrase, remember_minutes?}` | `Done`; `auth_failed` | WP31/32 |
| `LookupKeys {email, sources: [wkd, keys-openpgp-org]}` (nothing imported) | `KeyLookup {email, candidates (with armored key), failures}` | WP31 |
| `ListWindowsCertificates {store: personal \| other-people}` | `WindowsCertificates {certificates}`; `unsupported` off Windows | WP32 |
| `ImportWindowsCertificate {store, thumbprint}` (private key stays in the store) | `KeysImported` | WP32 |

`SecretProtection` is `keychain` (default: a random key in the OS
keychain, no prompt) or `passphrase {passphrase}`.

**Sending.**

- Per-identity settings are part of `Identity` (`crypto?: IdentityCrypto
  {technology: auto | openpgp | smime, sign, encrypt, openpgp_key,
  smime_certificate, autocrypt, prefer_encrypt: nopreference | mutual,
  protect_headers}`), stored in `accounts.kdl` as an identity child
  `crypto technology=… sign=… encrypt=… openpgp-key=… smime-cert=…
  autocrypt=… prefer-encrypt=… protect-headers=…` (only non-defaults are
  written; fingerprints normalized to uppercase hex without spaces) and
  saved with `UpdateAccount` like every identity setting — no separate
  request. `nail-config` reads, validates, writes (format-preserving) and
  documents it (JSON Schema, `userDocs/en/accounts-reference.md`).
- `SendCrypto {sign, encrypt, technology?, protect_headers?}`: the draft
  keeps the composer's toggles (`Draft.crypto?`, survives autosave);
  `SendDraft.crypto?` overrides them. Effective choice: request → draft →
  identity defaults. **Sending never falls back to plain text**: `no_key`
  when a recipient has no usable key, `locked` when the signing key needs
  its passphrase. Until WP35 wires signing and encryption, the core (and
  the UI mock) refuse any active choice with `unsupported` instead of
  sending unprotected (tested).
- `CheckRecipientKeys {identity_id, recipients, lookup}` →
  `RecipientKeys {report: {recipients: RecipientKeyStatus[] (best OpenPGP
  / S/MIME key with state usable | expired | revoked | rejected |
  cannot-encrypt | invalid-chain, Autocrypt peer, lookup state),
  openpgp / smime: {own_key, all_recipients}, autocrypt: disable |
  discourage | available | encrypt, suggested: SendCrypto}}`.
- Autocrypt setup messages: `SendAutocryptSetup {identity_id}` →
  `AutocryptSetupSent {outbox_id, passcode}` (shown once);
  `ImportAutocryptSetup {message_id, passcode}` → `KeysImported`.

**Data protection.** `GetSecuritySettings` / `UpdateSecuritySettings
{settings}` → `SecuritySettings {settings: {index_decrypted: AccountId[]
(opt-in, empty by default), key_lookup: off | on-demand (default) |
automatic, lookup_sources}}`. `GetVaultStatus` / `SetVaultEncryption
{enabled}` → `VaultStatus {status: {state: plain | encrypting | encrypted |
decrypting, phase: database | blobs | index, done, total,
key_in_keychain, error, message}}`.

**Events:** `KeysChanged` (refetch keys, recipient status and shown
message security), `VaultProgress {status}`, `SecuritySettingsChanged`.

MCP needs nothing new: the `encrypted` scope (ADR 0022) already guards
decrypted content; WP35 enforces it.

### Store schema v5 (`crates/nail-store`)

- `crypto_keys` (+ `crypto_key_emails`): metadata, the public part
  (binary packets / DER) and, for own keys, the **already encrypted**
  secret part or a reference to it (keychain entry, Windows thumbprint);
  unique per (technology, fingerprint). The store never holds a plain
  secret key.
- `crypto_trust`: user decisions keyed by (technology, fingerprint), apart
  from the key rows, so delete + re-import keeps them.
- `autocrypt_peers`: Autocrypt Level 1 peer state per (own account, peer
  address): `last_seen`, `autocrypt_timestamp`, key fingerprint,
  `prefer_encrypt`, gossip timestamp and fingerprint.
- `vault_state`: one row (state, phase, progress, keychain reference of the
  data key, key check value, times, last error) for a crash-safe, resumable
  migration.
- `message_security`: cached findings per message (status and trusted
  auth results JSON, never decrypted content; cascades with the message)
  for list badges and rule conditions.
- `app_settings`: small JSON settings (`SECURITY_SETTINGS_KEY`).

Accessors exist and are tested (`Store::upsert_key` keeps an existing
secret part when a public key is re-imported; `delete_key` refuses own keys
without the flag; `set_trust_decision`, `autocrypt_peer` /
`put_autocrypt_peer`, `vault_state` / `set_vault_state`,
`message_security` / `put_message_security`, `setting` / `set_setting`).
**Parallel packages must not add migrations**; a package that needs a
schema change reports it and WP35 appends v6.

### Crates and libraries

All pure Rust except the `windows` crate; `cargo tree` shows no OpenSSL
and no aws-lc. Licences checked with `cargo metadata`: MIT / Apache-2.0 /
BSD / ISC / Zlib / Unicode / bzip2-1.0.6 only (`r-efi` is triple-licensed
incl. MIT).

- rPGP `pgp` 0.20 pins the **stable RustCrypto generation** (digest 0.10,
  rsa 0.9, p256 / p384 0.13, ecdsa 0.16, aes 0.8, aes-gcm 0.10, rand 0.8).
  S/MIME and the vault use the same generation so every primitive is built
  once: `cms` 0.2.3 + `x509-cert` 0.2.5 (both with `builder`), `der` 0.7,
  `spki` 0.7, `const-oid` 0.9 (`db`), `cbc` 0.1, `sha1` 0.10,
  `sha2-legacy` (= `sha2` 0.10 with `oid`; the workspace `sha2` is 0.11 for
  ids and blob hashes), `p12-keystore` 0.2.1 (0.3 moved to the pre-release
  generation), `chacha20poly1305` 0.10 (XChaCha20-Poly1305), `zeroize`.
  `windows` 0.62 (`Win32_Security_Cryptography`) for the certificate
  store, a `cfg(windows)` dependency of `nail-smime`. `idna` 1.1 and
  `unicode-security` 0.1 for link safety in `nail-mime`.
- Stub crates with smoke tests that exercise the libraries on Windows
  MSVC: `nail-pgp` (generate Ed25519 + X25519, armor, encrypt / decrypt),
  `nail-smime` (self-signed P-256 certificate, CMS SignedData round trip,
  AES-GCM / AES-CBC, PKCS#12 reader, opening the Windows "MY" store),
  `nail-vault` (XChaCha20-Poly1305 with tamper detection, a tantivy
  `Directory`). Each crate works with `cargo test -p <crate>` alone
  (feature unification is not relied on).
- Expected shapes (the crates' module docs): `nail-pgp` and `nail-smime`
  are libraries over bytes plus the store's keyring; they return the
  contract types of `nail_api::security`. `nail-vault` owns the data key
  (keychain via `nail_api::engine::SecretStore`), blob / index encryption,
  the SQLite evaluation (an ADR if it needs anything beyond pure Rust) and
  the migration. None of them depends on `nail-core`; the `windows` calls
  need `unsafe`, kept in one module with a module-level allow and a safety
  comment per call.

### Collision-free slots

| Area | Slot | Owner |
| --- | --- | --- |
| OpenPGP / Autocrypt | `crates/nail-pgp/` | WP31 |
| S/MIME | `crates/nail-smime/` | WP32 |
| Encryption at rest | `crates/nail-vault/` | WP33 |
| Auth results, link safety | `crates/nail-mime/src/{auth_results,links}.rs` (declared, documented), `nail-mime` otherwise | WP33 |
| Store / search integration hooks | `crates/nail-store` code (not its schema; the v5 accessors above keep their signatures, WP31 / WP32 use them), `crates/nail-search` | WP33 |
| Test vectors | `crates/nail-testkit/corpus/crypto/pgp/` (WP31), `…/smime/` (WP32); read with `nail_testkit::crypto_fixture`; not demo mail (the corpus build and `load_mail_dir` skip `crypto/`) | WP31 / WP32 |
| UI | `ui/src/features/security/` (`index.ts`, `mock.ts` answering `SECURITY_REQUESTS`, `messages.ts`), registered in `features/registry.ts` | WP34 |
| Widget kinds | `crates/nail-config/src/widgets/security.rs` | WP34 |
| Strings | `locales/en/security.ftl` + empty `locales/de/security.ftl` | WP34 |

The core routes every M2a request to `nail-core/src/security.rs` (one
dispatch arm). It serves the security settings and the vault status
already; everything else answers `unsupported` until WP35. The UI mock
(`features/security/mock.ts`) behaves the same, so WP34 builds on real
answers where they exist and extends its own mock for the rest.

Integrator-owned during M2a (report, do not edit): `Cargo.toml`
(workspace dependencies are pre-declared — ask before adding more),
`Cargo.lock` changes beyond a package's own new dependency, `nail-api`,
`nail-store` migrations, `nail-core`, `nail-config` outside the widget
slot, `service.rs`, `App.tsx`, `src/api/mock.ts`, `src/state/uiStore.ts`,
`defaults/*.kdl`.

## Consequences

- WP31–WP34 compile against fixed types and generated TypeScript; the
  suites stay green in between (1000 Rust tests, 419 Vitest, 39 Playwright
  after this change).
- WP35 wires: decrypt / verify on read (and the `message_security` cache),
  sign / encrypt on send (replacing the `unsupported` guard), Autocrypt
  headers out and peer updates in, recipient key status and lookup, auth
  results and warnings in `MessageBody`, the vault switch with migration,
  the MCP `encrypted` grant, and the M2a Playwright scenarios.
- Rule forwards / redirects of encrypted mail must be re-encrypted or
  refused by WP35 (today the core cannot decrypt, so nothing leaks).
- Rules conditions on signature / encryption state (design "Filters &
  rules") are not part of this contract; `message_security` makes them
  cheap to add later.
