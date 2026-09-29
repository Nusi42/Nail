# pgp test vectors

Owned by M2a WP31 (`crates/nail-pgp`, ADR 0030). **Test material
only**: every secret key here is public and must never be used for real
mail.

## How they were made

All files were produced by **GnuPG 2.4.9** (the one shipped with Git for
Windows, see `generated-with.txt`) with `generate.sh` in this directory:

```
bash crates/nail-testkit/corpus/crypto/pgp/generate.sh      # regenerates everything in place
```

The script uses a throw-away `GNUPGHOME`, creates the keys with a faked
system time (stable dates), and wraps the OpenPGP data in PGP/MIME the way
Thunderbird 128 does it (boundaries `--------------…`, the RFC 3156
preamble, `OpenPGP_signature.asc` / `encrypted.asc` parts, protected headers
`protected-headers="v1"`). Signatures cover the exact CRLF bytes, so the
`.eml` files are marked `-text` in `.gitattributes`. Encrypted messages use
random session keys: regenerating changes them, and the tests must pass
with the new files.

## Keys

| File | Key | Notes |
| --- | --- | --- |
| `alice.pub.asc`, `alice.sec.asc` | Alice Example `<alice@example.org>`, Ed25519 + Cv25519 | secret key protected with the passphrase **`alice`** |
| `bob.pub.asc`, `bob.sec.asc` | Bob Example `<bob@example.net>`, Ed25519 + Cv25519 | secret key without passphrase |
| `carol.pub.asc`, `carol.sec.asc` | Carol Example `<carol@example.com>`, RSA 3072 + RSA 3072 subkey | no passphrase |
| `dave.pub.asc`, `dave.sec.asc` | Dave Example `<dave@example.org>`, Ed25519 + Cv25519 | created 2020-01-01, **expired** 2021-01-01 |
| `eve.pub.asc` | Eve Example `<eve@example.org>` | **revoked** (GnuPG's stored revocation certificate imported) |

All keys are created 2026-01-01 12:00 UTC unless noted and do not expire.

## Messages

| File | Content |
| --- | --- |
| `signed-alice.eml` | PGP/MIME `multipart/signed` by Alice (detached, SHA-256) |
| `signed-alice-tampered.eml` | the same with one word of the signed part changed (signature invalid) |
| `signed-carol-rsa.eml` | signed by Carol's RSA key; the signed part carries protected headers |
| `signed-dave-2020.eml` | signed by Dave on 2020-06-01, while his key was valid |
| `encrypted-to-bob.eml` | `multipart/encrypted` to Bob and Alice (SEIPD v1, GnuPG's default for these keys) |
| `encrypted-to-bob-ocb.eml` | the same to Bob only in GnuPG's OCB packet (tag 20, `--force-ocb`) |
| `encrypted-to-carol.eml` | encrypted to Carol only (Bob has no key for it) |
| `signed-encrypted-alice-to-bob.eml` | signed by Alice and encrypted to Bob and Alice, protected headers; the outer subject is `...`, the real one `Secret plans` |
| `inline-signed-alice.eml` | inline PGP, cleartext signature by Alice |
| `inline-encrypted-to-bob.eml` | inline PGP, signed by Alice and encrypted to Bob |
| `autocrypt-alice.eml` | plain mail with Alice's `Autocrypt` header (`prefer-encrypt=mutual`, minimal key from `--export-options export-minimal`) |
| `autocrypt-setup-bob.eml` | Autocrypt setup message with Bob's secret key (AES-128, iterated+salted SHA-256, `Passphrase-Format: numeric9x4`) |
| `autocrypt-setup-bob.passcode` | its passcode |

`generate.sh` and `generated-with.txt` are not test vectors; the demo corpus
skips this whole directory (`nail_testkit::crypto_fixture` reads it).
