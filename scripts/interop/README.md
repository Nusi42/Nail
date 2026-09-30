# Interop tests against real mail servers

Nail's IMAP, POP3 and SMTP engines (`nail-proto-imap`, `-pop3`, `-smtp`)
and the ManageSieve client (`nail-sieve`, M2c) against Dovecot 2.3 with
Pigeonhole and Stalwart (plan M2b WP44, ADR 0040; ManageSieve ADR 0049). CI runs them in
the job "interop" on `ubuntu-latest`; the in-process mock servers of
`nail-testkit` remain the main test bed.

This directory is its own Cargo workspace (so Linux does not build the
Tauri shell) with path dependencies on the engine crates. Copy the root
lock file first so the versions match:

```sh
cp Cargo.lock scripts/interop/Cargo.lock
bash scripts/interop/gen-certs.sh /tmp/nail-certs          # test CA + localhost certificate
bash scripts/interop/start-servers.sh /tmp/nail-certs      # Docker: Dovecot, Stalwart
NAIL_INTEROP=1 NAIL_INTEROP_CA=/tmp/nail-certs/ca.pem \
NAIL_INTEROP_SERVERS=dovecot=localhost:10000:sieve,stalwart=localhost:20000:smtp:sieve \
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
| `dovecot.conf` | complete Dovecot config: any user, password `nail-interop` |
| `stalwart.toml` | Stalwart config: `ada@example.org` / `nail-interop`, in-memory directory |
| `gen-certs.sh` | ECDSA test CA and server certificate for `localhost` |
| `start-servers.sh` | `docker run` both servers, wait for their ports |

Ports: Dovecot `10000 +` 143 / 993 / 110 / 995 / 4190, Stalwart `20000 +`
143 / 993 / 110 / 995 / 587 / 465 / 4190. A server runs the ManageSieve
tests when its entry in `NAIL_INTEROP_SERVERS` has the `sieve` feature:
STARTTLS login (and a refused wrong password), capabilities, `CHECKSCRIPT`
of a broken script, uploading Nail's script next to an active user script
(recorded and included), a renamed script that never includes Nail's old
one, and switching off (the user's script is active again).
