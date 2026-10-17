# Interop tests against real mail and calendar servers

Nail's IMAP, POP3 and SMTP engines (`nail-proto-imap`, `-pop3`, `-smtp`)
and the ManageSieve client (`nail-sieve`, M2c) against Dovecot 2.3 with
Pigeonhole and Stalwart (plan M2b WP44, ADR 0040; ManageSieve ADR 0049),
and the CalDAV / CardDAV client (`nail-dav`, M3a WP72, ADR 0060)
against Radicale 3. They need Docker and run on a machine that has it
(`NAIL_INTEROP=1`, below); the in-process mock servers of `nail-testkit` and `nail-testkit-net`
remain the main test bed.

This directory is its own Cargo workspace (so Linux does not build the
Tauri shell) with path dependencies on the engine crates. Copy the root
lock file first so the versions match:

```sh
cp Cargo.lock scripts/interop/Cargo.lock
bash scripts/interop/gen-certs.sh /tmp/nail-certs          # test CA + localhost certificate
bash scripts/interop/start-servers.sh /tmp/nail-certs      # Docker: Dovecot, Stalwart, Radicale
NAIL_INTEROP=1 NAIL_INTEROP_CA=/tmp/nail-certs/ca.pem \
NAIL_INTEROP_SERVERS=dovecot=localhost:10000:sieve,stalwart=localhost:20000:smtp:sieve \
NAIL_INTEROP_DAV=radicale=https://localhost:5232/ \
NAIL_INTEROP_USER=ada@example.org NAIL_INTEROP_PASSWORD=nail-interop \
cargo test --manifest-path scripts/interop/Cargo.toml -- --test-threads=1
```

Without `NAIL_INTEROP=1` the server tests return at once (the unit tests of
the configuration parser still run). With it, a missing variable fails the
run instead of skipping.

| File | What |
| --- | --- |
| `src/lib.rs` | environment configuration, PEM parsing, test messages |
| `tests/servers.rs` | the tests (IMAP, POP3, SMTP, ManageSieve; see the ADRs for the list) |
| `tests/dav.rs` | CalDAV / CardDAV tests (every server in `NAIL_INTEROP_DAV`, and always the mock) |
| `dovecot.conf` | complete Dovecot config: any user, password `nail-interop` |
| `stalwart.toml` | Stalwart config: `ada@example.org` / `nail-interop`, in-memory directory |
| `radicale.config` | Radicale 3 config (TLS on 5232, htpasswd, owner-only rights) |
| `radicale.users` | Radicale user `ada@example.org` / `nail-interop` (plain, test data) |
| `gen-certs.sh` | ECDSA test CA and server certificate for `localhost` |
| `start-servers.sh` | `docker run` the servers, wait for their ports (`NAIL_INTEROP_RADICALE=0` skips Radicale) |

Ports: Dovecot `10000 +` 143 / 993 / 110 / 995 / 4190, Stalwart `20000 +`
143 / 993 / 110 / 995 / 587 / 465 / 4190. A server runs the ManageSieve
tests when its entry in `NAIL_INTEROP_SERVERS` has the `sieve` feature:
STARTTLS login (and a refused wrong password), capabilities, `CHECKSCRIPT`
of a broken script, uploading Nail's script next to an active user script
(recorded and included), a renamed script that never includes Nail's old
one, and switching off (the user's script is active again).

## CalDAV / CardDAV (Radicale)

Radicale (image `tomsquest/docker-radicale`, override with
`RADICALE_IMAGE`) listens on `5232` with TLS from `gen-certs.sh`.
`NAIL_INTEROP_DAV` lists DAV servers as `name=url` (comma-separated); unset,
the DAV tests only run against the mock server. For every listed server
`tests/dav.rs` checks: discovery from the URL (well-known redirect,
principal, home sets), MKCALENDAR / extended MKCOL, conditional PUT, a
`calendar-query` time range, `sync-collection` before and after a second
client's change, a stale write that conflicts (the server's version comes
back), delete and its removal in the next sync, an unknown sync token
(restart from an initial sync), `addressbook-query` / multiget, the ctag /
ETag sync path, and a refused password. Each run works in fresh,
uniquely named collections and deletes them at the end.

Without Docker, Radicale also runs from PyPI on Windows for a local check
(a test server only; it is GPL and never shipped with Nail):

```sh
py -m venv <scratch>/venv && <scratch>/venv/Scripts/pip install radicale
# radicale.config with hosts = 127.0.0.1:5232 and local paths for the
# certificates, users file and storage folder
<scratch>/venv/Scripts/python -m radicale --config <scratch>/radicale.config
NAIL_INTEROP=1 NAIL_INTEROP_CA=<certs>/ca.pem NAIL_INTEROP_SERVERS=none=localhost:40000 \
NAIL_INTEROP_DAV=radicale=https://localhost:5232/ \
NAIL_INTEROP_USER=ada@example.org NAIL_INTEROP_PASSWORD=nail-interop \
cargo test --manifest-path scripts/interop/Cargo.toml --test dav
```
