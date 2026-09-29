# S/MIME test vectors

Owned by M2a WP32 (`crates/nail-smime`, ADR 0031). **Test material
only**: every private key here is public, and the PKCS#12 password of all
`.p12` files is `nail-test`. Never use any of it for real mail.

## How they were made

Everything is produced by [`generate.sh`](generate.sh) with the OpenSSL
command-line tool — OpenSSL 3.5.7 from Git for Windows
(`/mingw64/bin/openssl`), run with `bash generate.sh`.
OpenSSL is a fixture tool only; Nail never links it. The script needs
OpenSSL 3.4 or newer (`-not_before` / `-not_after`).

Regenerating replaces every key, certificate and message (new random keys,
serials stay). Tests pin no fingerprints, but they rely on the names,
validity periods and usages below.

Two details of the native Windows OpenSSL build are handled in the script:
Git Bash path conversion is switched off (`MSYS2_ARG_CONV_EXCL='*'`) so
`/O=…/CN=…` subjects survive, and OpenSSL writes S/MIME output in text mode,
which turns the CRLF of signed parts into CR CR LF; `fix_eol` restores CRLF
(signatures stay valid: they cover the canonical CRLF form). OpenSSL reads an
LF copy of `content.eml` and canonicalizes it itself.

## Certificates and keys (PEM; keys are unencrypted PKCS#8)

Validity is 2020-01-01 – 2125-01-01 unless noted (the end date is a
GeneralizedTime, so both time encodings are exercised).

| File | Subject | Key | Issuer | Purpose in tests |
| --- | --- | --- | --- | --- |
| `root-ca.crt/.key` | Nail Test Root CA | RSA 2048 | self | the trust anchor (`FixedTrustAnchors`) |
| `intermediate-ca.crt/.key` | Nail Test Email CA | EC P-384 | root (sha256WithRSA) | `pathlen:0`, EKU `emailProtection` |
| `alice.crt/.key` | Alice Example, `alice@example.org` | RSA 2048 | intermediate (ecdsa-with-SHA384) | signing, RSA key transport |
| `bob.crt/.key` | Bob Example, `bob@example.net` | EC P-256 | intermediate | ECDSA, ECDH key agreement |
| `carol.crt/.key` | Carol Expired | RSA 2048 | intermediate | **expired** 2021-01-01 |
| `dave.crt/.key` | Dave Server | RSA 2048 | intermediate | EKU `serverAuth` only: **wrong usage** |
| `erin.crt/.key` | Erin Encrypt Only | RSA 2048 | intermediate | key usage `keyEncipherment` only |
| `evil-root-ca.crt/.key` | Evil Test Root | RSA 2048 | self | an **untrusted** root |
| `mallory.crt/.key` | Mallory Untrusted | RSA 2048 | evil root | chain to the untrusted root |
| `sub-ca.crt/.key` | Nail Test Sub CA | EC P-256 | intermediate | CA below `pathlen:0` |
| `grace.crt/.key` | Grace Too Deep | RSA 2048 | sub CA | **path length** violation |
| `not-ca.crt/.key` | Not A CA | EC P-256 | intermediate | end-entity (CA:false) that issues |
| `heidi.crt/.key` | Heidi Bad Issuer | RSA 2048 | not-ca | issued by a **non-CA** |
| `ivan.crt/.key` | Ivan Critical | RSA 2048 | intermediate | unknown **critical extension** |
| `ada-work.crt/.key` | Ada Lovelace (Work), `ada@work.example` | RSA 2048 | intermediate | the `demo-multi` "work" account (M2a WP35 e2e: S/MIME round trip) |
| `ada-home.crt/.key` | Ada Lovelace (Home), `ada@home.example` | EC P-256 | intermediate | the `demo-multi` "home" account (ECDH recipient) |

## Bundles

| File | Content |
| --- | --- |
| `alice.p12` | alice's key and certificate + intermediate; OpenSSL 3 default (PBES2, AES-256-CBC, HMAC-SHA256) |
| `alice-legacy.p12` | the same with `-legacy` (3DES / RC2, SHA-1 MAC), like older Windows exports |
| `bob.p12` | bob's key and certificate + intermediate (PBES2) |
| `ada-work.p12`, `ada-home.p12` | the demo-multi accounts' keys and certificates + intermediate (PBES2), imported by `nail serve --seed demo-multi` |
| `alice-chain-p7c.der` | certs-only PKCS#7 (`.p7c`) with intermediate + alice, DER (named `.der` so `.gitattributes` keeps it binary) |

## Messages (CRLF, byte-exact; `.eml` is `-text` in `.gitattributes`)

`content.eml` is the inner MIME entity every message protects (a short
`text/plain` part, CRLF).

| File | Made with | Expected result |
| --- | --- | --- |
| `alice-signed-detached.eml` | `cms -sign` (multipart/signed, SHA-256, RSA PKCS#1 v1.5) | valid, chain valid |
| `alice-signed-opaque.eml` | `cms -sign -nodetach` (signed-data) | valid |
| `alice-signed-pss.eml` | `-keyopt rsa_padding_mode:pss` | valid (RSASSA-PSS) |
| `alice-signed-noattr.eml` | `-noattr` (no signed attributes) | valid, no signing time |
| `bob-signed-detached.eml` | ECDSA P-256 | valid |
| `carol-signed-expired.eml` | carol signs in 2026 | key expired, chain expired |
| `dave-signed-wrong-usage.eml` | dave (serverAuth) | signature valid, chain wrong usage |
| `mallory-signed-untrusted.eml` | mallory, `From: alice@example.org`, no chain included | valid signature, signer does not match From, chain incomplete / untrusted root |
| `alice-enc-cbc.eml` | `cms -encrypt -aes-256-cbc` to alice | enveloped-data, decrypts |
| `alice-enc-gcm.eml` | `-aes-256-gcm` to alice | authEnveloped-data, decrypts; bob has no key |
| `alice-enc-oaep.eml` | `-aes-256-gcm -keyopt rsa_padding_mode:oaep` | RSAES-OAEP (SHA-1) key transport |
| `bob-enc-gcm.eml` | `-aes-256-gcm` to bob | ECDH (`dhSinglePass-stdDH-sha*kdf`, AES key wrap) |
| `both-enc-gcm128.eml` | `-aes-128-gcm` to alice and bob | either can decrypt |
| `alice-signed-enc-bob.eml` | alice signs (detached), then encrypts to bob | decrypted + valid signature |

## Checking Nail's output with OpenSSL (manual interoperability check)

The test `mime::tests::interop_output_for_openssl` writes Nail-signed and
Nail-encrypted messages when `NAIL_SMIME_INTEROP_DIR` is set:

```sh
NAIL_SMIME_INTEROP_DIR=$TEMP/nail-interop cargo test -p nail-smime interop_output
cd crates/nail-testkit/corpus/crypto/smime
D=$(cygpath -m "$TEMP/nail-interop")   # Git Bash; use the plain path elsewhere
for f in nail-signed-detached nail-signed-opaque nail-signed-ecdsa; do
  MSYS2_ARG_CONV_EXCL='*' openssl cms -verify -in "$D/$f.eml" -CAfile root-ca.crt \
    -purpose smimesign -out "$D/$f.out"
done
openssl cms -decrypt -in "$D/nail-enc-gcm-alice.eml" -recip alice.crt -inkey alice.key
openssl cms -decrypt -in "$D/nail-enc-cbc-alice.eml" -recip alice.crt -inkey alice.key
openssl cms -decrypt -in "$D/nail-enc-gcm-bob.eml" -recip bob.crt -inkey bob.key
openssl cms -decrypt -in "$D/nail-enc-gcm128-bob.eml" -recip bob.crt -inkey bob.key
```

With OpenSSL 3.5.7 all three signatures verified and all four
messages decrypted (ADR 0031).
