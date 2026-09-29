# ADR 0034: M2a integration — end-to-end encryption and data protection in the core

Status: accepted · 2026-09-29 · M2a WP35

## Context

Wave M2a built OpenPGP / Autocrypt (`nail-pgp`, ADR 0030), S/MIME
(`nail-smime`, ADR 0031), data protection (`nail-vault`, sealed store
columns and blobs, the encrypted search index, `Authentication-Results`,
link safety; ADR 0032) and the security UI (ADR 0033) in parallel against
the contract of ADR 0029. WP35 wires them into the core and the UI and adds
the Playwright scenarios of plan section 7.

## Decisions

### Core state

- `Service` owns one `CryptoState` (`nail-core/src/crypto.rs`): the OpenPGP
  and S/MIME unlock caches (memory only), the S/MIME trust anchors and the
  HTTPS client for key lookups. Keyrings are built per call
  (`Service::pgp()`, `Service::smime()`) over the store and the secret
  store; all keyring work runs on the blocking pool (`Service::blocking`).
- `ServiceOptions` gains `trust_anchors` (default: the Windows Root / CA /
  Disallowed stores of the current user; none off Windows) and
  `key_lookup` (default: reqwest on rustls with the platform verifier,
  HTTPS only, at most three HTTPS redirects, 5 s connect / 10 s total,
  reading stops after `nail_pgp::discovery::MAX_RESPONSE_BYTES`).
- **Encryption at rest** (`vault.rs`, `VaultCtl`): `Service::open` opens the
  vault before anything reads messages or drafts. A missing or wrong data
  key leaves the vault locked: message reads answer `locked` / `crypto`
  (never empty data), `GetVaultStatus` reports the error, and the search
  index lives in memory so the sealed files are not mistaken for damage.
- **The search index is replaceable** (`search.rs`, `SearchSlot`): users
  take a short-lived `Arc` per use (`SearchSlot::get`), also the sync
  context. The vault migration runs database and blobs with the index open
  (`migrate_until(Index)`), then swaps in an in-memory stand-in (searches
  answer `unavailable` meanwhile), waits for the old index to be released,
  converts the files, reopens with the new directory wrapper and reindexes
  when mail arrived meanwhile. `Service::search_index()` now returns an
  `Arc`. `SetVaultEncryption` answers `conflict` while a migration runs;
  an interrupted migration resumes when the engines start.

### Reading

- `Service::message_body` (moved to `crypto.rs`): S/MIME first (top-level
  `classify`), then OpenPGP (`read_message`: PGP/MIME, nested signatures,
  inline PGP). The reader gets the decrypted message; for S/MIME the outer
  header fields are joined with the decrypted / unwrapped entity.
- **Nothing decrypted is written**: only the `SecurityStatus` and the
  trusted `AuthResults` go into the `message_security` cache. The stored
  subject of a header-protected message stays `...`.
- S/MIME signer certificates of valid signatures are remembered
  (`remember_certificates`, `KeysChanged`), so replies can be encrypted.
- **Sender authentication**: trusted `authserv-id`s come from the new
  account node `authserv-id "mx.example.net" …` in `accounts.kdl`
  (`AccountConfig.authserv_ids`, optional on the wire); without it the core
  learns the id from 20 recent inbox messages (80 % share, at most every
  10 minutes) and stores it in `app_settings` (`authserv-id/<account>`).
  *Superseded by ADR 0035:* only configured ids are trusted;
  `SuggestAuthservId` proposes one for the user to save.
- **Warnings** are computed after decryption, so decrypted HTML is checked.
  The reader's attachment list shows `AttachmentWarningMark`.
- Replying to or forwarding encrypted mail starts with encryption on in
  the same technology (`NewDraft` sets `Draft.crypto`); forwards carry the
  decrypted attachments.

### Sending

- `SendDraft` resolves the choice (request → draft → identity), checks it
  (`check_send`: own key, recipients' keys, signing key unlocked →
  `no_key` / `locked`) and stores the effective choice with the resolved
  technology on the draft; the sender applies exactly that. It never falls
  back to plain text (the old `unsupported` guard is gone; its test now
  expects `no_key`).
- `secure_outgoing` (on the built message): the `Autocrypt` header for
  identities with Autocrypt on and an own OpenPGP key (also on plain mail),
  then OpenPGP (`protect_message`: header protection per request /
  identity, `Autocrypt-Gossip` for more than one recipient, always also
  encrypted to the sender's own key) or S/MIME (detached signature,
  AES-256-GCM, the sender's own certificate always a recipient; S/MIME
  header protection is not produced).
- **Bcc** recipients of an encrypted message each get their own copy
  encrypted to the visible recipients plus themselves; the visible copy
  (the Sent copy) is not encrypted to them.
- The technology: request → identity preference → the first technology
  whose own key and recipients' keys all work (OpenPGP first) → the one
  with an own key → OpenPGP. The UI resolves it the same way.
- A draft with encryption on is **not uploaded** to the server's Drafts
  folder (it would sit there in plain text). *ADR 0035:* "on" includes
  identity defaults and an Autocrypt recommendation; an earlier plain copy
  is deleted.
- **Rules** never forward or answer encrypted mail (`unsupported`; a
  redirect sends the still-encrypted original and is allowed).
- `SendAutocryptSetup` builds the setup message (texts from
  `security.ftl`, now in the core's catalogs) and queues it through the
  outbox as a draft with the new `AutomaticMail::AutocryptSetup` (the
  prepared message in the draft's first attachment, sent unchanged to the
  identity itself).

### Arriving mail

`Service::secure_arrivals` runs on the rule inbox before the arrival
rules (new mail in Inbox / Other after the first sync, IMAP and POP3):
Autocrypt headers update the peer state; encrypted mail is decrypted in
memory for its `Autocrypt-Gossip` and, for accounts that opted in, for the
search index (`IndexPolicy::document`). Gossip is also applied when an
encrypted message is read. Opting in or out reindexes the account's
encrypted mail in the background; a full index rebuild restores the
decrypted documents of opted-in accounts.

### MCP

- The `encrypted` scope also covers inline PGP messages
  (`is_encrypted_mime` looks for an armored PGP message at a line start).
  *Superseded by ADR 0035:* the gate is the reader's own result
  (`CoreAccess::is_encrypted` → `Service::message_is_encrypted`).
- With decrypted indexing on, a client without `encrypted` gets search
  results without encrypted messages (restricted search path), so a match
  cannot reveal what they say (`McpData::index_holds_decrypted`).
- Attachments of encrypted messages are served from the raw message
  (ciphertext) even with the grant (follow-up).

### UI

- Layout: `security-message` and `security-warnings` above `message-view`
  in both reader layouts; `security-composer` below the composer; the four
  settings tabs; `security.autocrypt.import` in the reader toolbar. Theme:
  the parts of the ADR 0033 appendix (one selector per `part`).
- The composer saves only an explicit choice with the draft, but **sends
  what the toggles show** (`composerSendCrypto`: explicit, else the core's
  suggestion) — otherwise an Autocrypt suggestion shown as "Encrypt" would
  go out as the identity default. It refuses to send while
  `composerSendBlock()` names a problem (not while the key check is still
  running: the core checks again and refuses rather than send plain).
- The UI mock mirrors the core: `SendDraft` with signing / encryption is
  checked (`no_key` / `locked`) instead of refused.

### Demo and tests

- Every seeded run (`nail serve --seed …`, `nail-app --seed`) uses
  `demo::demo_options`: secrets in memory (never the OS keychain), the
  corpus' S/MIME test root as the only anchor, `OfflineKeyLookup` (every
  lookup finds nothing, no network).
- `DemoMulti::seed_security` gives both demo-multi accounts a generated
  OpenPGP key and an S/MIME certificate (`ada-work.p12`, `ada-home.p12`,
  new in the corpus, issued by the test CA; `generate.sh` makes them);
  the identities prefer encryption (`prefer-encrypt="mutual"`) and the
  accounts trust `authserv-id "mx.nail.example"`.
- Core integration tests `nail-core/tests/security.rs` (OpenPGP and S/MIME
  round trips through the mock SMTP / POP3 servers, header protection on
  the wire, Bcc copies, tampering, missing / locked keys, Autocrypt, sender
  authentication and warnings, key requests with an injected lookup,
  setup messages, decrypted indexing, the vault on / off with sealed blob
  files); Playwright `e2e/tests/security.spec.ts` (the nine scenarios of
  plan section 7, as seven tests).

## Consequences

- Nothing in M2a answers `unsupported` any more except the Windows store
  off Windows, lookups with the setting off, and automatic forwards /
  replies of encrypted mail.
- `rsa` 0.9 (RUSTSEC-2023-0071, Marvin timing side channel; no fixed
  release in this RustCrypto generation) is used by rPGP and `nail-smime`;
  both decrypt with blinding. The `cargo-deny` job of M2b (WP44) must
  record the exception with this rationale.
- Follow-ups: S/MIME inside `multipart/mixed` (list footers) is not
  unwrapped; S/MIME header protection; attachments of encrypted mail over
  MCP; partial sends (a failing Bcc copy after the first went out resends
  the first on retry); the message list shows `...` for header-protected
  mail; Autocrypt only learns from mail that arrives after the first sync
  of a folder; full-file SQLite encryption (ADR 0032); revocation checks
  and name constraints for S/MIME (ADR 0031); OpenPGP v6 (ADR 0030).
