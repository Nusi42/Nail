#!/usr/bin/env bash
# Regenerates the S/MIME test vectors in this directory (M2a WP32).
#
# Needs the OpenSSL command-line tool 3.4 or newer (for -not_before /
# -not_after); the checked-in files were made with OpenSSL 3.5.7 from Git
# for Windows (/mingw64/bin/openssl). OpenSSL is only a fixture tool here:
# Nail itself never links it.
#
# Everything is test material. The private keys and the PKCS#12 password
# ("nail-test") are public; never use them for real mail.
#
# Usage: bash generate.sh   (run from any directory; writes next to itself)
#
# Regenerating changes every key, certificate and message; the tests pin no
# fingerprints (they compare against these files), but they rely on the
# names, validity periods and usages below (see README.md).

set -euo pipefail
# Git Bash would rewrite "/CN=..." arguments into Windows paths.
export MSYS2_ARG_CONV_EXCL='*'

DIR="$(cd "$(dirname "$0")" && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
# A native (mingw) openssl needs a Windows path, since conversion is off.
if command -v cygpath >/dev/null; then WORK="$(cygpath -m "$WORK")"; fi
cd "$DIR"

PASS="nail-test"
# Long-lived test certificates; UTCTime ends in 2049, so this also exercises
# GeneralizedTime.
NB="20200101000000Z"
NA="21250101000000Z"

ext() { # name, extension lines...
  local name="$1"; shift
  { echo "[v3]"; printf '%s\n' "$@"; } > "$WORK/$name.cnf"
}

ext root \
  "basicConstraints=critical,CA:true" \
  "keyUsage=critical,keyCertSign,cRLSign" \
  "subjectKeyIdentifier=hash"
ext intermediate-ca \
  "basicConstraints=critical,CA:true,pathlen:0" \
  "keyUsage=critical,keyCertSign,cRLSign" \
  "extendedKeyUsage=emailProtection" \
  "subjectKeyIdentifier=hash" \
  "authorityKeyIdentifier=keyid"
leaf_ext() { # name, email, keyUsage, extendedKeyUsage
  ext "$1" \
    "basicConstraints=critical,CA:false" \
    "keyUsage=critical,$3" \
    "extendedKeyUsage=$4" \
    "subjectAltName=email:$2" \
    "subjectKeyIdentifier=hash" \
    "authorityKeyIdentifier=keyid"
}
leaf_ext alice alice@example.org "digitalSignature,nonRepudiation,keyEncipherment" emailProtection
leaf_ext bob bob@example.net "digitalSignature,keyAgreement" emailProtection
leaf_ext carol carol@example.org "digitalSignature,keyEncipherment" emailProtection
leaf_ext dave dave@example.org "digitalSignature,keyEncipherment" serverAuth
leaf_ext erin erin@example.org "keyEncipherment" emailProtection
leaf_ext mallory mallory@example.org "digitalSignature,keyEncipherment" emailProtection
# The demo-multi accounts of `nail serve --seed demo-multi` (M2a WP35, e2e).
leaf_ext ada-work ada@work.example "digitalSignature,nonRepudiation,keyEncipherment" emailProtection
leaf_ext ada-home ada@home.example "digitalSignature,keyAgreement" emailProtection

rsa_key() { openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out "$1.key" 2>/dev/null; }
ec_key() { openssl genpkey -algorithm EC -pkeyopt "ec_paramgen_curve:$2" -out "$1.key"; }

# issue name subject issuer-name serial not-before not-after digest
issue() {
  openssl req -new -key "$1.key" -subj "$2" -out "$WORK/$1.csr"
  openssl x509 -req -in "$WORK/$1.csr" -CA "$3.crt" -CAkey "$3.key" \
    -set_serial "$4" -not_before "$5" -not_after "$6" -"$7" \
    -extfile "$WORK/$1.cnf" -extensions v3 -out "$1.crt" 2>/dev/null
}

# --- Certificate authorities ------------------------------------------------
rsa_key root-ca
openssl req -new -key root-ca.key -subj "/O=Nail Test/CN=Nail Test Root CA" -out "$WORK/root.csr"
openssl x509 -req -in "$WORK/root.csr" -key root-ca.key -set_serial 1 \
  -not_before "$NB" -not_after "$NA" -sha256 \
  -extfile "$WORK/root.cnf" -extensions v3 -out root-ca.crt 2>/dev/null

ec_key intermediate-ca secp384r1
issue intermediate-ca "/O=Nail Test/CN=Nail Test Email CA" root-ca 2 "$NB" "$NA" sha256

# An unrelated, untrusted root.
rsa_key evil-root-ca
openssl req -new -key evil-root-ca.key -subj "/O=Evil Test/CN=Evil Test Root" -out "$WORK/evil.csr"
openssl x509 -req -in "$WORK/evil.csr" -key evil-root-ca.key -set_serial 1 \
  -not_before "$NB" -not_after "$NA" -sha256 \
  -extfile "$WORK/root.cnf" -extensions v3 -out evil-root-ca.crt 2>/dev/null

# --- End entities ----------------------------------------------------------
rsa_key alice
issue alice "/O=Nail Test/CN=Alice Example" intermediate-ca 1001 "$NB" "$NA" sha384
ec_key bob prime256v1
issue bob "/O=Nail Test/CN=Bob Example" intermediate-ca 1002 "$NB" "$NA" sha384
rsa_key carol # expired in 2021
issue carol "/O=Nail Test/CN=Carol Expired" intermediate-ca 1003 "$NB" 20210101000000Z sha384
rsa_key dave # server certificate, not for e-mail
issue dave "/O=Nail Test/CN=Dave Server" intermediate-ca 1004 "$NB" "$NA" sha384
rsa_key erin # may encrypt, may not sign
issue erin "/O=Nail Test/CN=Erin Encrypt Only" intermediate-ca 1005 "$NB" "$NA" sha384
rsa_key mallory # chains to the untrusted root
issue mallory "/O=Evil Test/CN=Mallory Untrusted" evil-root-ca 1006 "$NB" "$NA" sha256
rsa_key ada-work # demo-multi "work" account
issue ada-work "/O=Nail Test/CN=Ada Lovelace (Work)" intermediate-ca 1201 "$NB" "$NA" sha384
ec_key ada-home prime256v1 # demo-multi "home" account
issue ada-home "/O=Nail Test/CN=Ada Lovelace (Home)" intermediate-ca 1202 "$NB" "$NA" sha384

# --- Broken chains (chain validation tests) -----------------------------------
# A CA certificate below the intermediate, which has pathlen:0.
ext sub-ca \
  "basicConstraints=critical,CA:true" \
  "keyUsage=critical,keyCertSign,cRLSign" \
  "subjectKeyIdentifier=hash" \
  "authorityKeyIdentifier=keyid"
ec_key sub-ca prime256v1
issue sub-ca "/O=Nail Test/CN=Nail Test Sub CA" intermediate-ca 1100 "$NB" "$NA" sha384
leaf_ext grace grace@example.org "digitalSignature,keyEncipherment" emailProtection
rsa_key grace
issue grace "/O=Nail Test/CN=Grace Too Deep" sub-ca 1101 "$NB" "$NA" sha256
# An end-entity certificate (CA:false) that signs another certificate.
leaf_ext not-ca notca@example.org "digitalSignature,keyCertSign" emailProtection
ec_key not-ca prime256v1
issue not-ca "/O=Nail Test/CN=Not A CA" intermediate-ca 1102 "$NB" "$NA" sha384
leaf_ext heidi heidi@example.org "digitalSignature,keyEncipherment" emailProtection
rsa_key heidi
issue heidi "/O=Nail Test/CN=Heidi Bad Issuer" not-ca 1103 "$NB" "$NA" sha256
# A leaf with an unknown critical extension.
ext ivan \
  "basicConstraints=critical,CA:false" \
  "keyUsage=critical,digitalSignature,keyEncipherment" \
  "extendedKeyUsage=emailProtection" \
  "subjectAltName=email:ivan@example.org" \
  "1.3.6.1.4.1.55555.1=critical,ASN1:NULL"
rsa_key ivan
issue ivan "/O=Nail Test/CN=Ivan Critical" intermediate-ca 1104 "$NB" "$NA" sha384

# --- PKCS#12 --------------------------------------------------------------------
openssl pkcs12 -export -in alice.crt -inkey alice.key -certfile intermediate-ca.crt \
  -name "Alice Example" -passout "pass:$PASS" -out alice.p12
openssl pkcs12 -export -legacy -in alice.crt -inkey alice.key -certfile intermediate-ca.crt \
  -name "Alice Example" -passout "pass:$PASS" -out alice-legacy.p12
openssl pkcs12 -export -in bob.crt -inkey bob.key -certfile intermediate-ca.crt \
  -name "Bob Example" -passout "pass:$PASS" -out bob.p12
openssl pkcs12 -export -in ada-work.crt -inkey ada-work.key -certfile intermediate-ca.crt   -name "Ada Lovelace (Work)" -passout "pass:$PASS" -out ada-work.p12
openssl pkcs12 -export -in ada-home.crt -inkey ada-home.key -certfile intermediate-ca.crt   -name "Ada Lovelace (Home)" -passout "pass:$PASS" -out ada-home.p12
# A certs-only PKCS#7 bundle (.p7c) for certificate import (.der so .gitattributes keeps it binary).
openssl crl2pkcs7 -nocrl -certfile intermediate-ca.crt -certfile alice.crt -outform DER -out alice-chain-p7c.der

# --- Messages ------------------------------------------------------------------
# content.eml is the canonical (CRLF) inner entity every message protects.
# OpenSSL reads an LF copy: it canonicalizes to CRLF itself.
printf 'Content-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: 7bit\r\n\r\nHello Bob,\r\n\r\nthis message is protected with S/MIME.\r\n\r\nAlice\r\n' > content.eml
tr -d '\r' < content.eml > "$WORK/content-lf.txt"

# The native Windows build writes S/MIME output in text mode, so the CRLF it
# produces for the signed part becomes CR CR LF; restore plain CRLF.
fix_eol() { perl -0pi -e 's/\r\r\n/\r\n/g' "$1"; }

sign() { # out signer [extra openssl args...]
  local out="$1" signer="$2"; shift 2
  openssl cms -sign -in "$WORK/content-lf.txt" -signer "$signer.crt" -inkey "$signer.key" \
    -certfile intermediate-ca.crt -md sha256 \
    -from "$signer@example.org" -to bob@example.net -subject "S/MIME test $out" \
    "$@" -out "$out"
}
sign alice-signed-detached.eml alice
sign alice-signed-opaque.eml alice -nodetach
sign alice-signed-pss.eml alice -keyopt rsa_padding_mode:pss
sign alice-signed-noattr.eml alice -noattr
openssl cms -sign -in "$WORK/content-lf.txt" -signer bob.crt -inkey bob.key -certfile intermediate-ca.crt \
  -md sha256 -from bob@example.net -to alice@example.org -subject "S/MIME test ECDSA" \
  -out bob-signed-detached.eml
sign carol-signed-expired.eml carol
sign dave-signed-wrong-usage.eml dave
openssl cms -sign -in "$WORK/content-lf.txt" -signer mallory.crt -inkey mallory.key -md sha256 \
  -from alice@example.org -to bob@example.net -subject "S/MIME test untrusted" \
  -out mallory-signed-untrusted.eml

encrypt() { # out cipher recipients...
  local out="$1" cipher="$2"; shift 2
  openssl cms -encrypt -in "$WORK/content-lf.txt" "-$cipher" \
    -from alice@example.org -to bob@example.net -subject "S/MIME test $out" \
    -out "$out" "$@"
}
encrypt alice-enc-cbc.eml aes-256-cbc alice.crt
encrypt alice-enc-gcm.eml aes-256-gcm alice.crt
encrypt alice-enc-oaep.eml aes-256-gcm -recip alice.crt -keyopt rsa_padding_mode:oaep
encrypt bob-enc-gcm.eml aes-256-gcm bob.crt
encrypt both-enc-gcm128.eml aes-128-gcm alice.crt bob.crt

# Sign (detached, as a MIME entity without message headers), then encrypt.
openssl cms -sign -in "$WORK/content-lf.txt" -signer alice.crt -inkey alice.key \
  -certfile intermediate-ca.crt -md sha256 -out "$WORK/signed-entity.txt"
fix_eol "$WORK/signed-entity.txt"
openssl cms -encrypt -in "$WORK/signed-entity.txt" -binary -aes-256-gcm \
  -from alice@example.org -to bob@example.net -subject "S/MIME test signed and encrypted" \
  -out alice-signed-enc-bob.eml bob.crt

for f in *.eml; do fix_eol "$f"; done

echo "SHA-256 fingerprints (DER):"
for c in root-ca intermediate-ca alice bob carol dave erin mallory evil-root-ca sub-ca grace not-ca heidi ivan ada-work ada-home; do
  printf '%-16s %s\n' "$c" "$(openssl x509 -in "$c.crt" -noout -fingerprint -sha256 | sed 's/.*=//; s/://g')"
done
