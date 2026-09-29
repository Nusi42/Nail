# ADR 0014: Libraries for the M1a engines

Status: accepted · 2026-09-29 · M1a WP10

## Context

M1a adds SMTP, POP3, OAuth2, keychain storage, account discovery and
full-text search. WP10 creates the crates as stubs and pins the shared
dependencies in `[workspace.dependencies]`, so the parallel packages only
reference them. Each one was resolved and compiled on Windows (MSVC) with a
small test that exercises it without network access.

A cross-cutting constraint: TLS is rustls with the **`ring`** provider
everywhere, and the provider is chosen explicitly per connection (as the
IMAP client already does, ADR 0005). `aws-lc-rs` is kept out of the build:
it needs extra native tooling on Windows, and a second provider in the
process makes rustls' implicit default provider ambiguous.

## Decisions

| Crate | Version / features | Used by | Why |
| --- | --- | --- | --- |
| `lettre` | 0.11.23, no default features; `smtp-transport`, `pool`, `hostname`, `tokio1`, `tokio1-rustls`, `ring`, `rustls-platform-verifier` (the `tracing` feature was dropped in WP14: it logs AUTH payloads, ADR 0017) | `nail-proto-smtp` | The maintained Rust SMTP client: implicit TLS and STARTTLS, PLAIN / LOGIN / XOAUTH2, `send_raw` for our own MIME, connection pooling. No `builder`: MIME is built by Nail (`mail-builder` in M1b) |
| `keyring` | 4.2 (default `v1` feature) | `nail-auth` | Cross-platform keychain access: Windows Credential Manager, macOS Keychain, Secret Service. The v4 `v1` API picks the platform store itself; if Nail later needs more control (Android Keystore, iOS), switch to `keyring-core` plus explicit stores |
| `oauth2` | 5.0, no default features | `nail-auth` | PKCE, authorization URL, code exchange and refresh with typed tokens. Without its default `reqwest` 0.12 feature; `nail-auth` supplies the HTTP call through our `reqwest` 0.13 (oauth2's async client trait accepts a closure), so only one HTTP stack is built |
| `reqwest` | 0.13.5, no default features; `rustls-no-provider`, `json`, `charset`, `http2` | `nail-auth`, `nail-discovery` | Already in the lock file through Tauri. `rustls-no-provider` avoids aws-lc; clients are built with `tls_backend_preconfigured(<rustls config with ring + platform verifier>)` (`nail_auth::http_client()`), and tests pass a config with the mock server's root instead |
| `url` | 2.5 | `nail-auth`, `nail-discovery` | Redirect URIs, autoconfig URLs |
| `rustls-platform-verifier` | 0.7.1 | `nail-auth` (and the IMAP client) | The OS trust store, as for IMAP |
| `hickory-resolver` | 0.26.3 (default: system config, tokio) | `nail-discovery` | RFC 6186 SRV lookups (`_imaps._tcp`, `_submission._tcp`, …); the standard async resolver |
| `hickory-proto` | 0.26.3 | `nail-testkit-net` | Encoding DNS answers for the mock DNS layer |
| `quick-xml` | 0.42 | `nail-discovery` | Thunderbird autoconfig / ISPDB and Autodiscover XML |
| `tantivy` | 0.26.2 (default features) | `nail-search` | The full-text engine named in the design; in-process, incremental, fast |
| `base64` | 0.23 | `nail-proto-pop3`, `nail-testkit-net` | SASL payloads (same version the testkit already used) |

POP3 stays an in-house client (plan WP12): the protocol is small, the
available crates are unmaintained or blocking, and we need exact control
over UIDL bookkeeping and the leave-on-server policies.

## Consequences

- `cargo tree -i aws-lc-rs` must stay empty; adding a crate that enables
  aws-lc (for example `reqwest`'s default `rustls` feature) is a review
  finding.
- Build time grows mostly through `tantivy`; it is only linked into
  `nail-search` and, later, `nail-core`.
- The stub crates' tests double as a check that each dependency still
  builds and works as used.
