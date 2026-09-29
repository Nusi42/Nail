# ADR 0017 (WP12): Outgoing mail, POP3, OAuth2, keychain, discovery

Status: accepted · 2026-09-29 · M1a WP12

(Numbered with the `wp12` suffix because other M1a packages were writing
ADRs at the same time; the integrator may renumber.)

## Context

WP12 fills in the crates WP10 created as stubs (ADR 0012 "expected crate
shapes", libraries in ADR 0014): `nail-proto-smtp`, `nail-proto-pop3`,
`nail-auth`, `nail-discovery` and the mock servers in `nail-testkit-net`.
This ADR records how they use their libraries and where they deviate from
the obvious path.

## Decisions

### `nail-proto-smtp`: lettre's low-level connection, not its transport

`lettre::AsyncSmtpTransport` hides what Nail needs, so the crate drives
`lettre::transport::smtp::client::AsyncSmtpConnection` (TCP, rustls with
`ring`, STARTTLS upgrade, reply parsing, dot-stuffing) and does the rest
itself:

- **Full `EHLO` list.** lettre keeps only four known extensions from its own
  `EHLO`, so the session sends `EHLO` once more (allowed outside a
  transaction) and keeps every line: `SIZE` (limit checked before `MAIL`,
  declared as `SIZE=`), `8BITMIME`, `SMTPUTF8`, `DSN`, `AUTH`.
- **Own SASL.** lettre's `XOAUTH2` cannot answer the error challenge (a
  `334` with a JSON status) and has no `OAUTHBEARER`; `PLAIN`, `LOGIN`,
  `XOAUTH2` and `OAUTHBEARER` are therefore sent as raw commands, with the
  RFC 7628 dummy response after an error challenge, so a stale token ends in
  a clean `535` → `auth_failed`.
- **Own `MAIL` / `RCPT` parameters**: `BODY=8BITMIME` / `SMTPUTF8` when
  needed (refused with `unsupported` if the server lacks them), DSN
  `RET=HDRS` + `NOTIFY=FAILURE,DELAY` + `ORCPT` when `accounts.kdl` asks
  for it and the server offers it. A permanently refused recipient fails the
  whole submission (nothing is sent) and the error names the address.
- **Line ends.** Messages are normalized to CRLF and one trailing CRLF is
  removed, because lettre appends `CRLF . CRLF` itself (otherwise every
  message would gain an empty last line).
- **`EHLO` name** is `[127.0.0.1]` unless configured: the machine's host
  name would otherwise leak into the `Received` header of every mail.
- **One connection per message** (`SmtpTransport`), no pooling: submission
  volume is low and a fresh login picks up changed passwords and tokens.
  With OAuth2 a rejected token is refreshed (`force_refresh`) and the send
  retried once.
- **Errors** (`SmtpError` → `EngineError`): 5xx during `AUTH` →
  `auth_failed`; 530 at `MAIL` → `auth_failed`; 552 / `5.3.4` → `too_large`;
  other 5xx → `rejected` (not retryable); 4xx → `network` (retryable);
  connect / TLS / timeouts → `network` / `tls` (retryable). lettre reports
  handshake failures as connection errors; they are re-classified as `tls`
  by their message.

### `nail-proto-pop3`: in-house client

As planned (ADR 0014). Notable choices:

- `STLS` is attempted even when `CAPA` is missing and never falls back to
  plaintext; data buffered before the handshake is rejected; capabilities
  are asked again over TLS (RFC 2595).
- Password logins pick SASL `PLAIN` when offered, else `USER`/`PASS`, else
  `APOP`; `XOAUTH2` / `OAUTHBEARER` with the error challenge handled like
  SMTP. SASL initial responses longer than 255 octets are sent after a `+`
  continuation (RFC 5034), which OAuth tokens need.
- Login `-ERR`s with `[IN-USE]`, `[LOGIN-DELAY]` or `[SYS/TEMP]` are
  temporary (retryable), all others `auth_failed`.
- `DELE` only marks; deletions happen at `QUIT`. The leave-on-server
  policies are pure functions (`plan_downloads`, `plan_deletions`,
  `vanished`) over the server listing and the core's bookkeeping (UID →
  download time, locally deleted UIDs). A message that was never downloaded
  is never deleted. Servers without `UIDL` are reported as `unsupported`.
- `APOP` needs MD5: new dependency **`md-5` 0.11** (RustCrypto, same
  `digest` 0.11 as `sha2`), also used by the mock server.

### `nail-auth`

- **Keychain.** `KeychainSecretStore` maps `keychain:<service>/<name>` to
  the keyring entry (service, user) and stores UTF-8 bytes with
  `set_secret`. Windows limits one credential blob to 2560 bytes, which
  Microsoft refresh tokens and PEM certificates can exceed; values above
  2000 bytes are split into `<name>#1..n` entries behind a marker entry
  (chunks written first, stale chunks removed). The backend is a trait so
  the chunking is unit-tested; one test round-trips through the real OS
  keychain under a throw-away service name and cleans up.
- **Providers.** Built-in endpoints and mail scopes: Google
  (`https://mail.google.com/`, `access_type=offline`, `prompt=consent` to
  always get a refresh token) and Microsoft (`IMAP.AccessAsUser.All`,
  `POP.AccessAsUser.All`, `SMTP.Send` on `outlook.office.com`, plus
  `offline_access`). Client ids and secret references come only from
  `accounts.kdl`; a configured built-in may override endpoints (the tests
  point Google at the mock). Endpoints must be `https` (plain `http` only on
  loopback).
- **Flow.** `oauth2` 5 with PKCE S256; client credentials in the request
  body (`AuthType::RequestBody`: required for public clients, accepted by
  Google). Loopback redirect `http://127.0.0.1:<random port>/` (RFC 8252);
  requests for other paths or with a wrong `state` get `404` and do not end
  the flow. `access_denied` → `cancelled`; 10-minute default timeout. The
  page the browser shows afterwards is passed in by the caller
  (`CallbackPages`) because it is user-visible text that must be localized.
- **HTTP.** oauth2's HTTP client trait is implemented on a named type
  (`Fetcher`) with a boxed `Send` future; closures there trip up `Send`
  inference in the token manager's futures.
- **Tokens.** `TokenManager` stores only the refresh token in the keychain;
  access tokens live in memory and are refreshed 60 s before expiry,
  single-flight per secret; rotated refresh tokens are written back;
  `invalid_grant` → `auth_failed` (the account needs a new sign-in).
- Tokens, codes, passwords and client secrets never appear in logs, errors
  or `Debug` output (tests check the `Debug` forms).

### `nail-discovery`

- **Sources**, all at once, each bounded by a timeout (10 s default):
  `autoconfig.<domain>` (https, then http — Thunderbird does the same),
  `/.well-known/autoconfig` (https, then http), the ISPDB by domain and, if
  that is empty, by the domain of the best MX host (custom domains at
  Google / Microsoft; "last two labels" heuristic, no public suffix list),
  RFC 6186 SRV (`_imaps`, `_imap`, `_pop3s`, `_pop3`, and RFC 8314
  `_submissions` / `_submission`; target `.` = not offered), Autodiscover
  POX (`autodiscover.<domain>`, then `<domain>`; `redirectAddr` and
  https-only `redirectUrl`, depth 2). Host guessing (`imap.` / `mail.` /
  `pop.` / `smtp.` on the usual ports, TCP connect) runs only when every
  other source came back empty.
- HTTP redirects are followed manually (at most 3, to `https` only).
- **Ranking**: encrypted before unencrypted, then by source (autoconfig,
  well-known, ISPDB, SRV, Autodiscover, guess), keeping each source's order;
  identical server sets from several sources are listed once. Hosts of
  Google and Microsoft get OAuth2 with the matching provider.
- **Seams**: `HttpFetch`, `DnsLookup`, `HostProbe`. The real
  implementations can be pointed at mocks for tests and end-to-end runs:
  `ReqwestFetch::route_all_to(addr)` (every request to one local HTTP
  server, original host in `Host`), `HickoryDns::with_name_server(addr)`,
  `TcpProbe::with_overrides`. Depends on `nail-auth` for
  `provider_for_host` and the HTTP client.

### `nail-testkit-net`

Mock SMTP, POP3, OAuth2 (axum), HTTP (axum, routes by `Host` + path) and
DNS (UDP, hickory-proto messages; the zone is also usable in memory)
servers on `127.0.0.1:0`, recording what they receive and with fault
injection. TLS material and `MockTls` come from `nail-testkit`. New
dependencies here: `sha2` and `url` (workspace), `md-5`.

## Consequences

- lettre was first compiled with its `tracing` feature (ADR 0014). At
  `debug` level lettre then logs every line it writes, including `AUTH`
  payloads (base64 credentials and tokens) and message bodies. The
  integrator (WP14) dropped the feature from `[workspace.dependencies]`;
  it must not come back.
- The core (WP14) builds an account's sender with
  `SmtpConfig::from_endpoint` + `SmtpAuth::from_config` +
  `SmtpTransport::new`, and runs account tests with
  `nail_proto_smtp::test_login` / `nail_proto_pop3::test_login`.
- The OAuth callback pages need Fluent strings (two keys) in
  `locales/en/app.ftl`.
