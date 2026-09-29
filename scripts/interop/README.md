# Interop tests against real mail servers

Nail's IMAP, POP3 and SMTP engines (`nail-proto-imap`, `-pop3`, `-smtp`)
against Dovecot 2.3 and Stalwart (plan M2b WP44, ADR 0040). CI runs them in
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
NAIL_INTEROP_SERVERS=dovecot=localhost:10000,stalwart=localhost:20000:smtp \
NAIL_INTEROP_USER=ada@example.org NAIL_INTEROP_PASSWORD=nail-interop \
cargo test --manifest-path scripts/interop/Cargo.toml -- --test-threads=1
```

Without `NAIL_INTEROP=1` the server tests return at once (the unit tests of
the configuration parser still run). With it, a missing variable fails the
run instead of skipping.

| File | What |
| --- | --- |
| `src/lib.rs` | environment configuration, PEM parsing, test messages |
| `tests/servers.rs` | the tests (IMAP, POP3, SMTP; see the ADR for the list) |
| `dovecot.conf` | complete Dovecot config: any user, password `nail-interop` |
| `stalwart.toml` | Stalwart config: `ada@example.org` / `nail-interop`, in-memory directory |
| `gen-certs.sh` | ECDSA test CA and server certificate for `localhost` |
| `start-servers.sh` | `docker run` both servers, wait for their ports |

Ports: Dovecot `10000 +` 143 / 993 / 110 / 995 / 4190, Stalwart `20000 +`
143 / 993 / 110 / 995 / 587 / 465 / 4190. ManageSieve (4190) gets tests with
the M2c client (WP51).
