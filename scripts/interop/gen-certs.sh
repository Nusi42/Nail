#!/usr/bin/env bash
# Test CA and server certificate for the interop servers (Dovecot,
# Stalwart): ECDSA P-256, server name "localhost" (+ 127.0.0.1).
# The tests trust only this CA (NAIL_INTEROP_CA).
#
# Usage: scripts/interop/gen-certs.sh <out dir>
# Writes ca.pem, server.pem (leaf + CA chain) and server.key into <out dir>.
set -euo pipefail

out=${1:?usage: gen-certs.sh <out dir>}
mkdir -p "$out"
cd "$out"

openssl req -x509 -new -nodes -days 7 \
  -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 \
  -keyout ca.key -out ca.pem \
  -subj "/CN=Nail interop test CA" \
  -addext "basicConstraints=critical,CA:TRUE" \
  -addext "keyUsage=critical,keyCertSign,cRLSign"

openssl req -new -nodes \
  -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 \
  -keyout server.key -out server.csr \
  -subj "/CN=localhost"

cat > server.ext <<'EOF'
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature
extendedKeyUsage=serverAuth
subjectAltName=DNS:localhost,IP:127.0.0.1
EOF

openssl x509 -req -days 7 -in server.csr -CA ca.pem -CAkey ca.key -CAcreateserial \
  -out server.crt -extfile server.ext

cat server.crt ca.pem > server.pem
# The containers read these as their own (non-root) users.
chmod 644 server.pem server.key ca.pem
rm -f server.csr server.ext ca.key ca.srl
echo "certificates in $out"
