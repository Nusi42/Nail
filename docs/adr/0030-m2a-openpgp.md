# ADR 0030 (M2a WP31): OpenPGP and Autocrypt in `nail-pgp`

Status: accepted · 2026-09-29 · M2a WP31

## Context

Plan section 7, WP31: OpenPGP with rPGP (Sequoia is LGPL), a keyring with
protected secret keys, key generation, import / export, PGP/MIME (RFC 3156)
sign / encrypt / decrypt / verify, inline PGP read-only, header protection,
Autocrypt Level 1, WKD / keys.openpgp.org lookup behind injectable HTTP, and
the trust model of ADR 0029, tested against GnuPG / Thunderbird vectors.
The contract (ADR 0029) fixes the protocol types (`nail_api::security`) and
the store accessors (`nail-store/src/crypto.rs`, schema v5); WP31 adds no
migration and touches nothing outside `crates/nail-pgp` and
`corpus/crypto/pgp`.

## Decisions

### Crate shape

`nail-pgp` is a library over bytes plus the store's keyring. It does no
I/O of its own: the OS keychain is `nail_api::engine::SecretStore`, HTTP is
`discovery::HttpClient`, the clock can be pinned (`Keyring::at`). Modules:
`keyring`, `key` (parsing and validity), `compose` (outgoing), `read`
(incoming), `autocrypt`, `discovery`. `Error::code()` maps to the protocol
codes (`locked`, `no_key`, `crypto`, `auth_failed`, …) and
`From<Error> for ApiError` exists. No new dependency: everything uses the
crates WP30 declared (`pgp` 0.20, `rand`, `zeroize`, `sha1`, `base64`,
`chrono`, `mail-parser`); no OpenSSL, no GPL-family licence.

### Keyring and secret keys

- A row per primary key (`technology = openpgp`, fingerprint = primary
  fingerprint, `public_data` = binary transferable public key). Metadata
  (user ids, addresses, algorithm, dates, revoked, can sign / encrypt) comes
  from the validity analysis below.
- Own keys store the **binary transferable secret key, still
  S2K-protected** — the store never sees plain key material:
  - `passphrase`: iterated + salted SHA-256 S2K (rPGP's default count),
    AES-256 CFB. Signing and decrypting need `Keyring::unlock`; the
    passphrase then lives in `UnlockCache` (process memory, `Zeroizing`,
    optional expiry; `remember_minutes = null` = until Nail quits).
  - `keychain`: a random 256-bit secret (base64) in the OS keychain at
    `keychain:nail/openpgp/<FINGERPRINT>` protects the key with the same S2K
    format (one hash pass: the secret is already full-entropy). No prompt;
    the row alone is useless without the keychain entry. `Keyring::delete`
    removes the entry with the key.
  - Imported secret keys keep their own protection when they have one
    (`passphrase`), else they get a keychain secret; setup-message keys
    always get a keychain secret.
- Import merges by fingerprint (new user ids, subkeys, signatures,
  revocations; nothing removed) and **drops third-party certifications**
  (as keys.openpgp.org does) so flooded keys cannot bloat the keyring. A
  stored secret part is never replaced by a re-import. The source only
  changes upwards (`autocrypt` > `autocrypt-gossip` > everything else;
  own keys keep `generated` / `setup-message`).
- Generation: Ed25519 (legacy EdDSA, v4) signing/certifying primary +
  X25519 (ECDH Curve25519) encryption subkey — the format GnuPG 2.4 and
  Thunderbird use; RSA 4096 primary + subkey on request. Preferences AES-256,
  AES-128, SHA-256, SHA-512; SEIPD v1 feature. rPGP's builder has no expiry
  parameter, so an expiry is added by re-signing the user id and subkey
  bindings with a key-expiration subpacket before the key is protected.
- Trust (`keyring::effective_trust`): a stored decision (`verified`,
  `rejected`) wins; else own keys are `own`; else the source decides
  (`autocrypt`, `autocrypt-gossip`, otherwise `unknown`).

### Validity policy (`key::KeyInfo`)

rPGP parses but leaves policy to the application. Only self-signatures that
verify count; per component the newest one decides flags and expiry; key
and subkey revocations must verify; signing subkeys need a valid
back-signature; keys without any valid self-signature are rejected on
import. Without key flags the algorithm decides. Signing uses the newest
usable signing subkey, else the primary; encryption the newest usable
encryption subkey. Expired or revoked keys are never used to sign or
encrypt (`no_key`).

### Outgoing PGP/MIME (`compose::protect_message`)

Input is the complete message from `nail-compose`; its `Content-*` fields
and body become the protected entity, the rest stays outside; output is
CRLF throughout.

- Signed: `multipart/signed; micalg=pgp-sha256; protocol=
  "application/pgp-signature"`, binary detached SHA-256 signature over the
  exact entity, `OpenPGP_signature.asc` part (Thunderbird's layout). No
  preamble (it would be user-visible English text).
- Encrypted (optionally signed inside, one-pass, like `gpg --sign
  --encrypt` and Thunderbird): `multipart/encrypted`, SEIPD v1 with AES-256
  (readable by every implementation in use; v6 keys / SEIPD v2 are not
  produced), no compression.
- Header protection: Thunderbird's `protected-headers="v1"` plus RFC 9788's
  `hp="cipher"` / `hp="clear"`; From, To, Cc, Reply-To, Subject, Date,
  Message-ID, In-Reply-To and References are copied into the entity; when
  encrypting the outer `Subject` becomes `...` and `HP-Outer` fields record
  what was on the wire. `Autocrypt-Gossip` values go into the encrypted
  entity. `compose::with_header` adds the outer `Autocrypt` header.
- Checked with GnuPG 2.4.9: `gpg --verify` reports a good signature on
  Nail's `multipart/signed` output, and `gpg --decrypt` decrypts Nail's
  encrypted output with a good inner signature.

### Incoming (`read::read_message`)

Returns the `SecurityStatus` and a **display message**: the outer header
fields (protected ones replaced by the signed / decrypted values) around
the inner entity's content, without the signature part. The core renders
the display message with `nail-mime` and must not persist it unless the
account opted into indexing decrypted mail (ADR 0029).

- `multipart/encrypted` (top level only): each own key the message is
  addressed to is tried alone (rPGP's ring result is overwritten per
  session-key packet, so the successful key would otherwise be unknown).
  States: `decrypted`, `no-key` (not addressed to any own key), `locked`
  (only passphrase keys that are not unlocked), `failed` (malformed data,
  integrity check). GnuPG's OCB packet is accepted; malleable SED packets
  are not. `EncryptionInfo::algorithm` stays `null` (rPGP does not expose
  the session cipher after decryption).
- Nested `multipart/signed` inside the decrypted entity (RFC 3156 §6.1) is
  verified too; its signatures are appended.
- Signatures: the issuer (fingerprint or key id subpacket) is looked up
  among all stored keys and subkeys. `unknown-key` keeps the issuer
  fingerprint; `invalid` for a failed check, a component that may not sign,
  or a signature older than its key; `key-revoked`; `key-expired` when the
  signature was made after the key (or subkey) expired — a signature made
  before the expiry stays `valid`. `error` for an unparseable signature
  part. `signer_matches_from` compares with the protected `From` when there
  is one.
- Header protection: `protected` when the entity carries protected-headers
  / `hp`; `mismatch` when From / To / Cc address sets differ from the
  outer ones, or (signed-only mail) the outer subject does not contain the
  protected one.
- `multipart/mixed` whose first part is `multipart/signed` (list footers):
  verified, `partial = true`, no display message (the footer stays visible).
  Encrypted entities nested inside other multiparts are **not** decrypted
  (EFAIL-style wrapping).
- Inline PGP (read-only): `BEGIN PGP MESSAGE` / `BEGIN PGP SIGNED
  MESSAGE` blocks in the text/plain root or text/plain parts of
  mixed/alternative multiparts (at most 16 blocks per part, depth 4); the
  blocks are replaced by their plaintext in the display message.
  `partial = true` when there is other text around the block, several
  blocks, or sibling parts.

### Autocrypt Level 1 (`autocrypt`)

- Header parsing is strict (unknown non-`_` attributes invalidate it);
  written headers carry the minimal key (primary, the user id for `addr`,
  newest encryption subkey, their self-signatures), keydata folded at 76.
- `process_incoming` implements §2.3: one `From`, not `multipart/report`,
  exactly one valid header for the sender; effective date = min(Date, now);
  older than `autocrypt_timestamp` → no change; `last_seen` / timestamp /
  key / `prefer-encrypt` updates; gossip (§2.7) only from the decrypted
  part (`ReadOutcome::gossip`) and only about other recipients. Keys are
  stored with source `autocrypt` / `autocrypt-gossip`.
- `recommend` implements §2.4: `disable` without a usable (not expired,
  not revoked, not rejected) key; `discourage` for gossip-only or stale
  keys (Autocrypt key 35 days older than `last_seen`); `encrypt` when both
  sides say `mutual` or when replying to encrypted mail; the sender's own
  address is ignored. `recipient_status` fills the OpenPGP half of
  `RecipientKeyStatus`.
- Setup messages (§4.4): 36-digit `numeric9x4` passcode, AES-128 with
  iterated+salted SHA-256 S2K, `Passphrase-Format` / `Passphrase-Begin`
  armor headers, `Autocrypt-Prefer-Encrypt` in the key's armor. The
  explanatory text is passed in (`SetupMessageText`, from the Fluent
  catalogs); only the spec's subject is fixed. A wrong or malformed passcode
  is `auth_failed`.

### Discovery (`discovery`)

WKD advanced method, then the direct method when the advanced host fails or
has nothing (hash = z-base-32 of SHA-1 of the lowercased local part, `?l=`
parameter; verified against the draft's example), then keys.openpgp.org's
VKS `by-email`. 404/410 is "not found", not a failure. Responses over
1 MiB are refused. Only keys with a user id for the address are kept,
third-party certifications are dropped, secret material is never passed
on; candidates are deduplicated by fingerprint, valid keys first.
`mark_known` and `import_candidate` (source `wkd` / `keyserver`) are
separate, so the lookup future holds no store reference.

### Test vectors

GnuPG 2.4.9 is available locally (Git for Windows), so the vectors are
real GnuPG output: `corpus/crypto/pgp/generate.sh` creates the keys (faked
dates; expired and revoked keys included) and the messages in Thunderbird's
MIME layout; the README lists every file. Tests: unit tests per module,
`tests/interop.rs` (GnuPG vectors: valid, tampered, unknown signer, RSA,
signature before expiry, SEIPD v1 and OCB decryption, wrong key, locked /
unlocked passphrase keys, protected subject, inline signed / encrypted,
EFAIL-style wrapping), `tests/roundtrip.rs` (Nail's own output: sign,
encrypt, sign + encrypt with header protection, tampered body and
ciphertext, wrong key, expired and revoked signers, header mismatch, list
footer, nested sign-then-encrypt), `tests/autocrypt.rs` (peer rules,
gossip, recommendation, one exchange enables encryption, GnuPG setup
message, setup round trip).

## Consequences

- WP35 wires the requests: `ListKeys` / `GetKey` / `ImportKeys` /
  `ExportPublicKey` / `GenerateKey` / `DeleteKey` / `SetKeyTrust` /
  `UnlockKey` → `Keyring`; `LookupKeys` → `discovery::lookup` +
  `mark_known` (with an `HttpClient` over the core's reqwest: HTTPS only,
  timeouts, 1 MiB cap); `CheckRecipientKeys` → `autocrypt::recipient_status`
  + `recommend` + `Keyring::own_key_for`; `SendAutocryptSetup` /
  `ImportAutocryptSetup` → `autocrypt::{new_passcode, create_setup_message,
  import_setup}`. Sending: `with_header` (Autocrypt) then
  `protect_message` with `Keyring::encryption_keys` (+ the own key) and
  `Keyring::unlocked_key` (`locked` → ask for the passphrase; never send
  unprotected). Reading: `read_message`, render `display`, cache only the
  status, then `autocrypt::process_incoming` with `IncomingMessage::from_raw`
  and the outcome's gossip; emit `KeysChanged` after key changes.
- RSA 4096 generation takes seconds in debug builds; the unit test
  generates RSA 2048 through the same code path.
- Not done here: OpenPGP v6 keys / SEIPD v2 output, designated revokers,
  the RFC 9788 "legacy display" part, `EncryptionInfo::algorithm`.
