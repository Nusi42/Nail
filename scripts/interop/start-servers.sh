#!/usr/bin/env bash
# Starts Dovecot, Stalwart and Radicale in Docker for Nail's interop tests
# and waits until every port answers. Used by the CI job "interop"; works
# on any Linux machine with Docker.
#
# Usage: scripts/interop/start-servers.sh <cert dir>
#   <cert dir>: output of gen-certs.sh.
# Environment: DOVECOT_IMAGE, STALWART_IMAGE, RADICALE_IMAGE (defaults
# below); NAIL_INTEROP_RADICALE=0 skips Radicale.
# Ports: Dovecot on 10000 + {143,993,110,995,4190},
#        Stalwart on 20000 + {143,993,110,995,587,465,4190},
#        Radicale (CalDAV / CardDAV over TLS) on 5232.
# Then run the tests with (see scripts/interop/README.md):
#   NAIL_INTEROP=1 NAIL_INTEROP_CA=<cert dir>/ca.pem \
#   NAIL_INTEROP_SERVERS=dovecot=localhost:10000:sieve,stalwart=localhost:20000:smtp:sieve \
#   NAIL_INTEROP_DAV=radicale=https://localhost:5232/ \
#   NAIL_INTEROP_USER=ada@example.org NAIL_INTEROP_PASSWORD=nail-interop \
#   cargo test --manifest-path scripts/interop/Cargo.toml
set -euo pipefail

certs=$(cd "${1:?usage: start-servers.sh <cert dir>}" && pwd)
here=$(cd "$(dirname "$0")" && pwd)
dovecot_image=${DOVECOT_IMAGE:-dovecot/dovecot:2.3.21}
stalwart_image=${STALWART_IMAGE:-stalwartlabs/stalwart:latest}
radicale_image=${RADICALE_IMAGE:-tomsquest/docker-radicale:latest}
radicale=${NAIL_INTEROP_RADICALE:-1}

docker rm -f nail-dovecot nail-stalwart nail-radicale >/dev/null 2>&1 || true

docker run -d --name nail-dovecot \
  -p 10143:143 -p 10993:993 -p 10110:110 -p 10995:995 -p 14190:4190 \
  -v "$here/dovecot.conf:/etc/dovecot/dovecot.conf:ro" \
  -v "$certs:/etc/dovecot/certs:ro" \
  "$dovecot_image"

docker run -d --name nail-stalwart \
  -p 20143:143 -p 20993:993 -p 20110:110 -p 20995:995 \
  -p 20587:587 -p 20465:465 -p 24190:4190 \
  -v "$here/stalwart.toml:/opt/stalwart/etc/config.toml:ro" \
  -v "$certs:/opt/stalwart/etc/certs:ro" \
  "$stalwart_image"

if [ "$radicale" = 1 ]; then
  docker run -d --name nail-radicale \
    -p 5232:5232 \
    -v "$here/radicale.config:/config/config:ro" \
    -v "$here/radicale.users:/config/users:ro" \
    -v "$certs:/certs:ro" \
    "$radicale_image"
fi

wait_for() {
  local port=$1 deadline=$((SECONDS + 90))
  until (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null; do
    if ((SECONDS > deadline)); then
      echo "port $port did not open" >&2
      docker logs nail-dovecot >&2 || true
      docker logs nail-stalwart >&2 || true
      docker logs nail-radicale >&2 || true
      return 1
    fi
    sleep 1
  done
  echo "port $port is up"
}

ports="10143 10993 10110 10995 14190 20143 20993 20110 20995 20587 20465 24190"
if [ "$radicale" = 1 ]; then
  ports="$ports 5232"
fi
for port in $ports; do
  wait_for "$port"
done
