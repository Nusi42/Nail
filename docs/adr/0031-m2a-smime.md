# ADR 0031: S/MIME (`nail-smime`)

Status: accepted · 2026-09-29 · M2a WP32

## Context

Plan section 7, WP32: S/MIME 4.0 (RFC 8551) on RustCrypto (`cms`,
`x509-cert`) — sign (detached `multipart/signed` and opaque), encrypt
(AES-GCM, AES-CBC for compatibility), decrypt, verify; PKCS#12 import;
the Windows certificate store (list, import, keys that stay in the store);
chain validation against the Windows trust store behind a trait; key-usage
and expiry checks; revocation reported as "not checked". The contract
(ADR 0029) fixes the protocol types (`SecurityStatus`, `SignatureInfo`,
`CertificateChain`, `CryptoKey`, `WindowsCertificate`, …) and the store's
keyring (schema v5). No OpenSSL, no GPL-family licences.

## Decisions

### Crate shape

`nail-smime` is a library over bytes plus the store's keyring; it has no
dependency on `nail-core` and no user-visible strings (errors are English
diagnostics with an `ErrorCode`; the UI localizes by code).

| Module | Role |
| --- | --- |
| `mime` | S/MIME MIME layer: `classify`, `sign_entity` (detached / opaque), `encrypt_entity`, `read` (unwraps up to 4 layers in any order, returns the innermost entity and a `SecurityStatus`) |
| `signed` | CMS `SignedData`: `sign`, `verify` (per-signer outcome) |
| `enveloped` | `EnvelopedData` (AES-CBC) and `AuthEnvelopedData` (RFC 5083, AES-GCM): `encrypt`, `inspect`, `decrypt` |
| `ber` | BER → DER normalization of CMS input |
| `cert` | certificate parsing (PEM, DER, PKCS#7 bundles, base64), identity, extensions, usage |
| `chain` | path building and validation, `TrustAnchors` trait, `FixedTrustAnchors` |
| `keys` | public keys, `SignScheme`, `RsaPadding`, `PrivateKeyOps` trait, `SoftwareKey` |
| `pkcs12` | `.p12` / `.pfx` import |
| `keyring` | the store's keyring: import, list, export, delete, unlock, private-key protection, `read_message`, signer / recipient selection |
| `windows` (`cfg(windows)`) | store listing / lookup, `WindowsKey` (CNG), `WindowsTrustAnchors`; the crate's only `unsafe` code |

### Algorithms

* **Signing** (Nail's output): signed attributes `contentType`,
  `signingTime` (UTCTime until 2049), `messageDigest`, `smimeCapabilities`
  (AES-256-GCM, AES-128-GCM, AES-256-CBC, AES-128-CBC); SHA-256 (SHA-384 for
  P-384 keys); `rsaEncryption` (PKCS#1 v1.5, what every client accepts) or
  `ecdsa-with-SHA*`; signer certificate plus intermediates included;
  `IssuerAndSerialNumber` signer id. Every signature is verified against
  the certificate before it leaves (catches a key that does not match its
  certificate or a faulty key provider). Detached signing canonicalizes the
  entity to CRLF first; the composer must hand over 7-bit-safe transfer
  encodings.
* **Verification** accepts RSA PKCS#1 v1.5, RSASSA-PSS (MGF1 over the same
  hash) and ECDSA P-256 / P-384 with SHA-256/384/512. **SHA-1 signatures
  are `unsupported`, never `valid`** (RFC 8551 section 2.1). Signed
  attributes are hashed exactly as sent (taken from the raw DER, since the
  decoded SET is re-sorted). A detached signature that fails over the part
  as received is retried over its CRLF-canonical form (gateways that strip
  CR). States: `valid`, `invalid` (digest or signature mismatch, content
  type mismatch), `unknown-key` (certificate neither in the message nor the
  keyring), `key-expired` (signature fine, certificate not valid at the
  signing time), `unsupported`, `error` (malformed). The certificate is
  judged at the signing time when the signature carries one that is not in
  the future (one day of skew), else now.
* **Encryption** (Nail's output): default AES-256-GCM in
  `AuthEnvelopedData` (12-byte nonce, 16-byte tag); AES-128-GCM, and
  AES-128/256-CBC in `EnvelopedData` for old clients (`ContentCipher`).
  Recipients: RSA key transport with RSAES-PKCS1-v1_5 (OAEP is decrypted
  but not produced yet: Outlook / older Thunderbird compatibility), EC
  recipients with ephemeral-static ECDH `dhSinglePass-stdDH-sha256kdf`
  (P-256) / `sha384kdf` (P-384), X9.63 KDF over `ECC-CMS-SharedInfo`, AES
  key wrap (RFC 3394). A certificate that may not encrypt (key usage / EKU)
  is refused (`no_key`). The sender adds its own certificate to keep a
  readable copy (the caller's choice).
* **Decryption** accepts key transport (PKCS#1 v1.5, OAEP with
  SHA-1/256/384/512, empty label) and ECDH with SHA-1/256/384/512 KDFs and
  AES-128/192/256 wrap; AES-CBC (128/192/256) and AES-GCM (128/256, 12-byte
  nonce, 16-byte tag; `authAttrs` as AAD). 3DES / RC2 content is reported
  `unsupported`. Outcomes: decrypted, `no-key` (no own certificate is a
  recipient), `locked` (the matching key needs its passphrase), `failed`
  (tampered, wrong key, damaged), `unsupported`.
* **BER**: Thunderbird / NSS, Outlook and `openssl -stream` produce
  indefinite lengths and chunked OCTET STRINGs; `ber::normalize` rewrites
  them into DER without changing values (DER passes through byte for byte),
  including the IMPLICIT `encryptedContent [0]` of an
  `EncryptedContentInfo`.

### Chain validation

Nail builds the path itself (leaf → message / keyring / system
intermediates → anchor, preferring the issuer the authority key identifier
names, at most 8 certificates) and checks: signatures of every link (SHA-1
certificate signatures are not accepted), validity at the chosen time for
every certificate, basic constraints `cA` and `keyCertSign` on every CA
below the anchor, path length constraints, extended key usage on CAs
(Windows semantics: an intermediate with EKU must allow
`emailProtection` or `anyExtendedKeyUsage`), the leaf's key usage / EKU for
the purpose (signing: `digitalSignature` or `nonRepudiation`; encryption:
`keyEncipherment` for RSA, `keyAgreement` for EC; EKU `emailProtection` or
any), unknown critical extensions (→ `invalid`), and explicit distrust.
Result precedence: `invalid` > `untrusted-root` / `incomplete` >
`expired` > `wrong-usage` > `valid`. Revocation is always `not-checked`.

Trust anchors are a trait (`TrustAnchors`): `anchors_for(subject)`,
`intermediates_for(subject)`, `is_distrusted(cert)`. On Windows,
`WindowsTrustAnchors` reads the current user's `Root` (which includes the
machine's roots), `CA` and `Disallowed` stores; tests use
`FixedTrustAnchors` with the corpus test root. We validate ourselves rather
than call `CertGetCertificateChain`, so results are identical in tests and
the app and no network retrieval (AIA, root auto-update) happens behind
the user's back.

Not evaluated in M2 (conservative: never silently valid): **name
constraints and policy constraints** — both are critical by RFC 5280, so a
chain through such a CA is `invalid`; missing intermediates are not fetched
(`incomplete`); revocation (OCSP / CRL) is not checked.

### Private keys

* PKCS#12 with `p12-keystore` 0.2.1 (pure Rust, MIT / Apache-2.0): PBES2 /
  AES files and legacy 3DES / RC2 ones (its default `pbes1` feature). A MAC
  or padding failure is `auth_failed`; the key must match the leaf
  certificate; the file's other certificates are imported as keyring
  entries (chain building).
* Stored keys are sealed with AES-256-GCM, associated data
  `NSM1 | mode | fingerprint`: **keychain** (default) — a random 256-bit key
  in the OS keychain at `keychain:nail/smime-key/<fingerprint>`, no prompt;
  **passphrase** — PBKDF2-HMAC-SHA256, 600 000 iterations (stored with the
  blob), 16-byte salt; a wrong passphrase is `auth_failed`. Unlocked keys
  live in an `UnlockCache` (per process; optional expiry from
  `UnlockKey.remember_minutes`). The store never holds a plain key.
* **Windows store**: `ImportWindowsCertificate` stores the certificate and
  `secret_storage = windows-store` with the SHA-1 thumbprint; `WindowsKey`
  opens the key per operation with `CryptAcquireCertificatePrivateKey`
  (`ONLY_NCRYPT | CACHE`) and signs (`NCryptSignHash`, PKCS#1 / ECDSA with
  `r‖s` → DER), decrypts (`NCryptDecrypt`, PKCS#1 / OAEP) and agrees
  (`NCryptSecretAgreement` + `TRUNCATE` raw secret, byte order reversed).
  The key never enters Nail's memory; Windows may show its own prompt
  (smart card PIN, strong key protection). Legacy CryptoAPI-only keys are
  `unsupported`. Tests use real CNG keys from `PFXImportCertStore` with
  `PKCS12_NO_PERSIST_KEY` (ephemeral, nothing written to the user's
  stores).

### Keyring conventions

`technology = smime`, fingerprint = SHA-256 of the DER certificate,
`user_ids = [subject DN]`, emails from `rfc822Name` SANs and PKCS #9
`emailAddress`, `created_at` / `expires_at` = validity, `issuer` /
`serial` (uppercase hex). Re-importing keeps the original source and the
secret part. Trust: a user decision wins (`verified` / `rejected`), own
keys are `own`, otherwise `unknown` — a valid chain shows in `chain`, not
in the trust level. Recipient selection ranks `usable` > `invalid-chain` >
`expired` > `cannot-encrypt` > `rejected`, newest first; a `verified`
decision accepts an untrusted or incomplete chain.
`encryption_certificates` returns `no_key` naming every address without a
usable certificate (never a partial or plain send). `read_message` fills
`key_id` / `trust` and writes nothing; `remember_certificates` stores the
certificates of **valid** signatures (source `message`) and the CAs the
message carried, so replies can be encrypted.

### Dependencies

Workspace dependencies as declared (ADR 0029) with extra features in
`nail-smime/Cargo.toml` only: `der` (`derive`, `oid`, `pem`), `p256` /
`p384` (`ecdh`), `cbc` (`alloc`, `block-padding`), `sha1` (`oid`). Two
direct dependencies not declared in the workspace, both already in
`Cargo.lock` through rPGP / p12-keystore (MIT OR Apache-2.0): `aes-kw`
0.2.1 (RFC 3394) and `pbkdf2` 0.12.2. `cargo tree -i openssl-sys` and
`-i aws-lc-sys` stay empty.

Known advisory: `rsa` 0.9 (RUSTSEC-2023-0071, Marvin timing side channel,
no fixed release in this generation). Nail decrypts with blinding
(`decrypt_blinded`) and signs with `sign_with_rng`; an offline mail client
is not a decryption oracle, but the security review should record it (rPGP
carries the same dependency).

### Tests and vectors

`crates/nail-testkit/corpus/crypto/smime/` holds OpenSSL 3.5.7-generated
CA, users, broken chains, PKCS#12 files and messages, with `generate.sh`
and a README listing every file. 88 unit tests cover: interop with every
OpenSSL vector (detached, opaque, PSS, no signed attributes, ECDSA,
CBC / GCM / OAEP / ECDH / two recipients, sign-then-encrypt), Nail's own
round trips for every mode and cipher, tampering (signed text, signature
bytes, encapsulated content, GCM ciphertext and tag, CBC padding), wrong
recipient, locked keys, expired / not-yet-valid / wrong-usage / untrusted /
incomplete / forged / non-CA / path-length / critical-extension /
distrusted chains, BER normalization, PKCS#12 (modern, legacy, wrong
password), keyring protection (keychain loss, AAD binding, passphrase
expiry), recipient selection and trust decisions, and real CNG keys
(RSA sign / decrypt / OAEP, ECDSA, ECDH) plus the system Root store.

Reverse interoperability was checked manually: OpenSSL 3.5.7 verified
Nail's detached, opaque and ECDSA signatures and decrypted Nail's
AES-256-GCM, AES-256-CBC (RSA) and AES-256/128-GCM (ECDH) messages (the
corpus README has the commands; `NAIL_SMIME_INTEROP_DIR` makes the test
write the files).

## Consequences

* WP35 wires the requests (see the package report): `ImportPkcs12`,
  `ImportKeys` for certificates, `ListKeys` / `GetKey` / `ExportPublicKey` /
  `DeleteKey` / `UnlockKey`, `ListWindowsCertificates` /
  `ImportWindowsCertificate`, the read path (`Keyring::read_message`,
  then `remember_certificates`), the send path (`Keyring::signer` +
  `mime::sign_entity`, `Keyring::encryption_certificates` + own
  certificate + `mime::encrypt_entity`), and the S/MIME half of
  `CheckRecipientKeys` (`Keyring::recipient_key`), with
  `WindowsTrustAnchors` in the app and `FixedTrustAnchors` (corpus test
  root) for demo / e2e runs.
* Header protection for S/MIME (RFC 8551 `message/rfc822` wrapping,
  draft-ietf-lamps-header-protection) is not produced or interpreted:
  `header_protection` is always `none`, and `read` handles top-level S/MIME
  (callers pass a body part for nested S/MIME; `partial` stays false).
* Follow-ups: produce RSAES-OAEP once client support is confirmed; name
  constraints; revocation (M3+); PKCS#11 smart cards (M4).
