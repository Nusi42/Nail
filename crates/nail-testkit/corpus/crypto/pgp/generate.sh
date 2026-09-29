#!/usr/bin/env bash
# Regenerates the OpenPGP interoperability vectors in this directory with
# GnuPG (see README.md). Usage:
#
#   bash crates/nail-testkit/corpus/crypto/pgp/generate.sh [out-dir]
#
# Keys are created with a faked system time so their dates are stable;
# encrypted messages still differ on every run (random session keys), and
# regenerating replaces every file, so the Rust tests must keep passing
# afterwards. Messages are wrapped in PGP/MIME the way Thunderbird 128
# does it (boundaries, preamble, part headers, protected headers v1).
set -euo pipefail

OUT=${1:-$(cd "$(dirname "$0")" && pwd)}
GNUPGHOME=$(mktemp -d)
export GNUPGHOME
trap 'gpgconf --kill gpg-agent >/dev/null 2>&1 || true; rm -rf "$GNUPGHOME"' EXIT
WORK=$GNUPGHOME/work
mkdir -p "$WORK"

KEY_TIME=20260101T120000
g() { gpg --batch --yes --quiet --pinentry-mode loopback --no-emit-version --no-comments \
    --trust-model always "$@"; }

# --- Keys --------------------------------------------------------------------
# alice: Ed25519 + Cv25519, secret key protected with the passphrase "alice".
g --faked-system-time $KEY_TIME --passphrase alice \
    --quick-gen-key 'Alice Example <alice@example.org>' future-default default never
# bob: Ed25519 + Cv25519, secret key without passphrase.
g --faked-system-time $KEY_TIME --passphrase '' \
    --quick-gen-key 'Bob Example <bob@example.net>' future-default default never
# carol: RSA 3072 primary + RSA 3072 encryption subkey, no passphrase.
g --faked-system-time $KEY_TIME --passphrase '' \
    --quick-gen-key 'Carol Example <carol@example.com>' rsa3072 sign,cert never
CAROL=$(g --list-keys --with-colons carol@example.com | awk -F: '/^fpr/ {print $10; exit}')
g --faked-system-time $KEY_TIME --passphrase '' --quick-add-key "$CAROL" rsa3072 encr never
# dave: created 2020-01-01, expired 2021-01-01.
g --faked-system-time 20200101T120000 --passphrase '' \
    --quick-gen-key 'Dave Example <dave@example.org>' future-default default 1y
# eve: revoked with the revocation certificate GnuPG stores.
g --faked-system-time $KEY_TIME --passphrase '' \
    --quick-gen-key 'Eve Example <eve@example.org>' future-default default never
EVE=$(g --list-keys --with-colons eve@example.org | awk -F: '/^fpr/ {print $10; exit}')
sed 's/^:-----/-----/' "$GNUPGHOME/openpgp-revocs.d/$EVE.rev" >"$WORK/eve.rev"
g --faked-system-time 20260102T120000 --import "$WORK/eve.rev"

for who in alice bob carol dave eve; do
    email=$(g --list-keys --with-colons "$who" | awk -F: '/^uid/ {print $10; exit}' | sed 's/.*<\(.*\)>/\1/')
    g --armor --export "$email" >"$OUT/$who.pub.asc"
done
g --armor --passphrase alice --export-secret-keys alice@example.org >"$OUT/alice.sec.asc"
g --armor --passphrase '' --export-secret-keys bob@example.net >"$OUT/bob.sec.asc"
g --armor --passphrase '' --export-secret-keys carol@example.com >"$OUT/carol.sec.asc"
g --armor --passphrase '' --export-secret-keys dave@example.org >"$OUT/dave.sec.asc"

# --- Helpers -----------------------------------------------------------------
crlf() { sed 's/$/\r/'; }

# Outer headers of a message from $1 to $2 with subject $3 and id $4.
headers() {
    printf 'From: %s\nTo: %s\nSubject: %s\nDate: Thu, 01 Jan 2026 12:00:00 +0000\nMessage-ID: <%s@example.org>\nMIME-Version: 1.0\n' \
        "$1" "$2" "$3" "$4"
}

# Thunderbird's multipart/signed wrapper around the part in $1 (CRLF,
# signed as is) with the armored signature in $2.
signed_body() {
    printf 'Content-Type: multipart/signed; micalg=pgp-sha256;\n protocol="application/pgp-signature";\n boundary="------------sig0Bnd4ry"\n\n' | crlf
    printf 'This is an OpenPGP/MIME signed message (RFC 4880 and 3156)\n--------------sig0Bnd4ry\n' | crlf
    cat "$1"
    printf '\n--------------sig0Bnd4ry\nContent-Type: application/pgp-signature; name="OpenPGP_signature.asc"\nContent-Description: OpenPGP digital signature\nContent-Disposition: attachment; filename="OpenPGP_signature.asc"\n\n' | crlf
    crlf <"$2"
    printf '\n--------------sig0Bnd4ry--\n' | crlf
}

# Thunderbird's multipart/encrypted wrapper around the armored message in $1.
encrypted_body() {
    printf 'Content-Type: multipart/encrypted;\n protocol="application/pgp-encrypted";\n boundary="------------enc0Bnd4ry"\n\n' | crlf
    printf 'This is an OpenPGP/MIME encrypted message (RFC 4880 and 3156)\n--------------enc0Bnd4ry\nContent-Type: application/pgp-encrypted\nContent-Description: PGP/MIME version identification\n\nVersion: 1\n\n--------------enc0Bnd4ry\nContent-Type: application/octet-stream; name="encrypted.asc"\nContent-Description: OpenPGP encrypted message\nContent-Disposition: inline; filename="encrypted.asc"\n\n' | crlf
    crlf <"$1"
    printf '\n--------------enc0Bnd4ry--\n' | crlf
}

# A plain text part (the signed / encrypted content), CRLF, no trailing
# line break after the last line (the delimiter's CRLF follows).
text_part() {
    printf 'Content-Type: text/plain; charset=UTF-8; format=flowed\nContent-Transfer-Encoding: 7bit\n\n%s\n' "$1" | crlf
}

# Thunderbird's protected-headers part: multipart/mixed with the real
# headers, holding one text part.
protected_part() {
    printf 'Content-Type: multipart/mixed; boundary="------------inner0Bnd4ry";\n protected-headers="v1"\nFrom: %s\nTo: %s\nMessage-ID: <%s@example.org>\nSubject: %s\n\n--------------inner0Bnd4ry\n' \
        "$1" "$2" "$4" "$3" | crlf
    text_part "$5"
    printf '\n--------------inner0Bnd4ry--\n' | crlf
}

strip_last_crlf() { head -c -2 "$1" >"$1.tmp" && mv "$1.tmp" "$1"; }

ALICE='Alice Example <alice@example.org>'
BOB='Bob Example <bob@example.net>'

# --- PGP/MIME signed ---------------------------------------------------------
text_part 'Hello Bob, this message is signed by Alice.' >"$WORK/signed.part"
strip_last_crlf "$WORK/signed.part"
g --passphrase alice -u alice@example.org --digest-algo SHA256 --detach-sign --armor \
    -o "$WORK/signed.sig" "$WORK/signed.part"
{ headers "$ALICE" "$BOB" 'Signed by Alice' signed-alice | crlf; signed_body "$WORK/signed.part" "$WORK/signed.sig"; } \
    >"$OUT/signed-alice.eml"
# The same with one changed word in the signed part.
sed -b 's/signed by Alice/signed by Mallory/' "$OUT/signed-alice.eml" >"$OUT/signed-alice-tampered.eml"

# Signed with protected headers (Thunderbird default), by carol's RSA key.
protected_part 'Carol Example <carol@example.com>' "$BOB" 'RSA signed' signed-carol \
    'Hello Bob, this message is signed with an RSA key.' >"$WORK/carol.part"
strip_last_crlf "$WORK/carol.part"
g -u carol@example.com --digest-algo SHA256 --detach-sign --armor -o "$WORK/carol.sig" "$WORK/carol.part"
{ headers 'Carol Example <carol@example.com>' "$BOB" 'RSA signed' signed-carol | crlf; signed_body "$WORK/carol.part" "$WORK/carol.sig"; } \
    >"$OUT/signed-carol-rsa.eml"

# Signed by dave in 2020, while his key was valid (it expired in 2021).
text_part 'Signed before my key expired.' >"$WORK/dave.part"
strip_last_crlf "$WORK/dave.part"
g --faked-system-time 20200601T120000 -u dave@example.org --digest-algo SHA256 --detach-sign --armor \
    -o "$WORK/dave.sig" "$WORK/dave.part"
{ headers 'Dave Example <dave@example.org>' "$BOB" 'Old signature' signed-dave | crlf; signed_body "$WORK/dave.part" "$WORK/dave.sig"; } \
    >"$OUT/signed-dave-2020.eml"

# --- PGP/MIME encrypted ------------------------------------------------------
# Encrypted to bob (and alice) with GnuPG's defaults (SEIPD v1 with MDC
# for these keys).
text_part 'Only Bob can read this.' >"$WORK/enc.part"
g -r bob@example.net -r alice@example.org --encrypt --armor -o "$WORK/enc.asc" "$WORK/enc.part"
{ headers "$ALICE" "$BOB" 'Encrypted' encrypted-bob | crlf; encrypted_body "$WORK/enc.asc"; } \
    >"$OUT/encrypted-to-bob.eml"
# The same in GnuPG's own OCB packet (tag 20), which GnuPG 2.4 sends when
# every recipient key announces its AEAD feature.
g --force-ocb -r bob@example.net --encrypt --armor -o "$WORK/enc-ocb.asc" "$WORK/enc.part"
{ headers "$ALICE" "$BOB" 'Encrypted' encrypted-bob-ocb | crlf; encrypted_body "$WORK/enc-ocb.asc"; } \
    >"$OUT/encrypted-to-bob-ocb.eml"
# Encrypted to carol only (bob has no key for it).
g -r carol@example.com --encrypt --armor -o "$WORK/enc-carol.asc" "$WORK/enc.part"
{ headers "$ALICE" 'Carol Example <carol@example.com>' 'Encrypted' encrypted-carol | crlf; encrypted_body "$WORK/enc-carol.asc"; } \
    >"$OUT/encrypted-to-carol.eml"

# Signed + encrypted by alice to bob, with protected headers: the outer
# subject is "...", the real one is inside (Thunderbird's default).
protected_part "$ALICE" "$BOB" 'Secret plans' signed-encrypted \
    'The meeting is at noon. Signed and encrypted by Alice.' >"$WORK/se.part"
g --passphrase alice -u alice@example.org --digest-algo SHA256 -r bob@example.net -r alice@example.org \
    --sign --encrypt --armor -o "$WORK/se.asc" "$WORK/se.part"
{ headers "$ALICE" "$BOB" '...' signed-encrypted | crlf; encrypted_body "$WORK/se.asc"; } \
    >"$OUT/signed-encrypted-alice-to-bob.eml"

# --- Inline PGP (read-only) --------------------------------------------------
printf 'Hello Bob,\n\nthis is an inline signed message.\n\nAlice\n' >"$WORK/inline.txt"
g --passphrase alice -u alice@example.org --digest-algo SHA256 --clearsign -o "$WORK/inline.asc" "$WORK/inline.txt"
{ headers "$ALICE" "$BOB" 'Inline signed' inline-signed | crlf
  printf 'Content-Type: text/plain; charset=UTF-8\nContent-Transfer-Encoding: 7bit\n\n' | crlf
  crlf <"$WORK/inline.asc"; } >"$OUT/inline-signed-alice.eml"
g --passphrase alice -u alice@example.org -r bob@example.net --sign --encrypt --armor \
    -o "$WORK/inline-enc.asc" "$WORK/inline.txt"
{ headers "$ALICE" "$BOB" 'Inline encrypted' inline-encrypted | crlf
  printf 'Content-Type: text/plain; charset=UTF-8\nContent-Transfer-Encoding: 7bit\n\n' | crlf
  crlf <"$WORK/inline-enc.asc"; } >"$OUT/inline-encrypted-to-bob.eml"

# --- Autocrypt ---------------------------------------------------------------
# A plain message from alice carrying her Autocrypt header (minimal key:
# primary, user id, encryption subkey, self-signatures).
KEYDATA=$(g --export-options export-minimal --export alice@example.org | base64 -w 0 | fold -w 72 | sed 's/^/ /')
{ printf 'From: %s\nTo: %s\nSubject: Hi with Autocrypt\nDate: Thu, 01 Jan 2026 12:00:00 +0000\nMessage-ID: <autocrypt-alice@example.org>\nMIME-Version: 1.0\nAutocrypt: addr=alice@example.org; prefer-encrypt=mutual; keydata=\n%s\nContent-Type: text/plain; charset=UTF-8\n\nHi Bob, my key is in the header.\n' \
    "$ALICE" "$BOB" "$KEYDATA"; } | crlf >"$OUT/autocrypt-alice.eml"

# Autocrypt setup message with bob's secret key, passcode below (numeric9x4).
PASSCODE=1742-0185-6197-1303-7016-1653-3213-1936-9431
g --armor --passphrase '' --export-secret-keys bob@example.net |
    sed '1a Autocrypt-Prefer-Encrypt: mutual' >"$WORK/setup.txt"
g --symmetric --armor --cipher-algo AES128 --s2k-mode 3 --s2k-digest-algo SHA256 --rfc4880 \
    --passphrase "$PASSCODE" -o "$WORK/setup.asc" "$WORK/setup.txt"
sed '1a Passphrase-Format: numeric9x4\nPassphrase-Begin: 17' "$WORK/setup.asc" >"$WORK/setup-headers.asc"
{ printf 'From: %s\nTo: %s\nSubject: Autocrypt Setup Message\nDate: Thu, 01 Jan 2026 12:00:00 +0000\nMessage-ID: <setup-bob@example.net>\nMIME-Version: 1.0\nAutocrypt-Setup-Message: v1\nContent-Type: multipart/mixed; boundary="setup0Bnd4ry"\n\n--setup0Bnd4ry\nContent-Type: text/plain; charset=UTF-8\n\nThis message contains all information to transfer your Autocrypt\nsettings along with your secret key securely from your original\ndevice.\n\n--setup0Bnd4ry\nContent-Type: application/autocrypt-setup\nContent-Disposition: attachment; filename="autocrypt-setup-message.html"\n\n<html><body><p>This is the Autocrypt setup file.</p><pre>\n' "$BOB" "$BOB"
  cat "$WORK/setup-headers.asc"
  printf '</pre></body></html>\n\n--setup0Bnd4ry--\n'; } | crlf >"$OUT/autocrypt-setup-bob.eml"
printf '%s\n' "$PASSCODE" >"$OUT/autocrypt-setup-bob.passcode"

gpg --version | head -1 >"$OUT/generated-with.txt"
echo "done: $OUT"
