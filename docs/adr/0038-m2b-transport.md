# ADR 0038: Transport and browser hardening, sandbox origin, remote-content proxy

Status: accepted · 2026-09-29 · M2b WP42

## Context

Plan section 7, WP42, and design.md "Remote UI security" ask for TLS 1.3
(1.2 only with forward-secret AEAD), a self-signed certificate with a
fingerprint pinned at pairing, ACME wired but locked, strict browser headers,
Host / Origin checks, mail HTML on a separate sandbox origin, and a
remote-content proxy that is never an open proxy and cannot be used for SSRF.
The contract (ADR 0036) gives WP42 `crates/nail-server` and
`crates/nail-proxy`. The loopback `nail serve` must keep working unchanged.

## Decisions

### `nail-proxy` (library; no `nail-core` dependency)

* **Tokens.** `base64url(body) "." base64url(HMAC-SHA-256)`, body = version,
  purpose (`resource` | `render`), expiry (Unix seconds), message id,
  payload (the URL for `resource`). The MAC input is prefixed with a domain
  label. The key (`ProxyKey`, 32 random bytes) is **per install**, kept in
  the keychain at `keychain:nail/proxy/signing-key`
  (`ProxyKey::load_or_create`), wiped on drop, never printed. Lifetimes:
  12 h (resource), 1 h (render). Verification checks the MAC first
  (constant time), then purpose, expiry, and the URL rules again. All token
  failures are `404` towards clients.
* **Not an open proxy.** Only the core can mint tokens, and it mints them
  only for URLs the rewriting hook finds in a message it renders
  (`Signer::rewriter(message_id)` returns the `nail-proxy:` URL or `None`).
  An optional `TargetCheck` lets the core re-confirm at fetch time that the
  message is still allowed (allow-list entries can be removed).
* **URL rules** (`policy::check_url`, at signing and on every hop):
  `http`/`https`, no user info, no local-only names (`localhost`, single
  labels, `.local`, `.localhost`, `.internal`, `.home.arpa`, `.lan`, …), no
  Fetch-standard "bad port", at most 4096 bytes. Literal hosts in every
  spelling (decimal, hex, octal, short, percent-encoded, bracketed IPv6)
  are normalized by the URL parser before the address rules run.
* **Address rules** (`policy::is_public_ip`): IPv4 blocks 0/8, 10/8,
  100.64/10, 127/8, 169.254/16, 172.16/12, 192.0.0/24, 192.0.2/24,
  192.31.196/24, 192.52.193/24, 192.88.99/24, 192.168/16, 192.175.48/24,
  198.18/15, 198.51.100/24, 203.0.113/24, 224/4, 240/4. IPv6 must be global
  unicast (2000::/3) minus 2001::/23 (incl. Teredo), 2001:db8::/32 and
  3fff::/20; IPv4-mapped, NAT64 (64:ff9b::/96) and 6to4 (2002::/16)
  addresses are unwrapped and checked as IPv4.
* **Fetching.** Per hop: resolve once (`Resolver`; default the OS resolver
  via `tokio::net::lookup_host`), refuse the hop if **any** answer is not
  public, then connect through a reqwest client whose only resolution is
  the checked addresses (`resolve_to_addrs` plus a resolver that refuses
  every other name) — no second lookup, so DNS rebinding cannot swap the
  target; the response's peer address is checked again. Redirects are
  never followed by reqwest: each `Location` is a new hop (at most 5).
  No system proxy, no cookie store, no `Referer`, `Accept-Encoding:
  identity`, a fixed `User-Agent`; nothing from the browser's request is
  forwarded. Limits: 5 MiB, 15 s total, 5 s connect / DNS. Allowed types:
  raster images (PNG, APNG, JPEG, GIF, WebP, AVIF, BMP, ICO), fonts (WOFF,
  WOFF2, TTF, OTF) and CSS — no SVG, HTML or scripts. The hickory
  resolver the WP40 stub declared is not used (the OS resolver honours the
  system configuration on Windows; the check-then-pin design does not need
  DNS internals), so it was dropped from the crate.
* **Serving** (`Proxy::serve`, shared by `nail-server` and the desktop
  protocol handler): errors map to statuses (`ProxyError::status`: token
  404, refused destination 403, type 415, size 413, timeout 504, else 502);
  `RESPONSE_HEADERS` make every answer inert (`nosniff`, `default-src
  'none'; sandbox`, `no-referrer`, CORP `cross-origin`).
* **Tests** only against loopback mocks. The feature `test-support`
  (dev-dependencies only) adds `Fetcher::allow_loopback_port_for_tests`
  (exactly one loopback port counts as public) and `testing` (a raw
  HTTP/HTTPS upstream that records requests, a scripted resolver).

### `nail-server`

* **API.** `Server::bind` is unchanged (loopback, plain HTTP; the
  existing tests and `nail-cli` compile as before). `Server::bind_with(service,
  options, TransportOptions)` adds TLS (required for non-loopback:
  `ServerError::TlsRequired`), host names, allowed networks, the proxy, the
  sandbox origin, the header profile and ACME challenges.
  `Server::sandbox_origin()` tells WP45 what the app must frame.
* **TLS** (`tls`): rustls with *ring*, TLS 1.3 + TLS 1.2 restricted to
  ECDHE + AES-GCM / ChaCha20-Poly1305 (rustls has no static-RSA or CBC
  suites), server order, ALPN `http/1.1` (the WebSocket upgrade). A custom
  `axum::serve::Listener` (`tls::Incoming`) drops peers outside
  `allowed_networks` (loopback always allowed) before reading a byte and
  runs handshakes in their own tasks with a 10 s deadline and at most 64
  in flight. `ConnectInfo<ClientAddr>` carries the peer address for WP41's
  rate limits.
* **Certificates** (`cert`): ECDSA P-256, self-signed, 397 days, names from
  `subject_names(settings, computer_name)` (host name or computer name,
  `localhost`, loopback addresses, a specific bind address), SHA-256
  fingerprint as uppercase hex (`RemoteCertificateInfo`). Stored as ADR 0036
  says: the record (chain, fingerprint, names, validity, `keychain:`
  reference) under `REMOTE_UI_CERTIFICATE_KEY`, the PKCS#8 key in the
  keychain at `keychain:nail/remote-ui/certificate-key`.
  `load_or_create_self_signed` reuses it across restarts (pinned
  fingerprints stay valid) and replaces it when the names change, within 30
  days of expiry, or when the key is missing or does not match;
  `regenerate_self_signed` backs `RegenerateRemoteCertificate` (WP45).
* **ACME** (`acme`): `issue` runs HTTP-01 with instant-acme (account, order,
  challenge through `AcmeChallenges` served at
  `/.well-known/acme-challenge/`, CSR from rcgen, chain → `ServerIdentity`),
  but its first step is `check_available`, which returns
  `InternetModeLocked(pentest-pending)` while `internet_mode_lock()` is set
  — always in this build — before any network access. The settings checks
  after the lock are unit-tested with an unlocked lock value. HTTP-01 also
  needs port 80 to reach Nail; recorded for the internet-mode gate.
* **Headers** (`headers`): CSP `default-src 'self'; script-src 'self'` (no
  inline script, no eval), `style-src 'self' 'unsafe-inline'` (the compiled
  theme is a `<style>` element), `object-src 'none'; base-uri 'none';
  form-action 'none'; frame-ancestors 'none'`, `frame-src` limited to the
  app and the sandbox origin; Trusted Types `require-trusted-types-for
  'script'; trusted-types nail nail-reader`; COOP `same-origin`, COEP
  `require-corp`, CORP `same-origin` (unless a route sets its own),
  `X-Frame-Options: DENY`, `nosniff`, `no-referrer`, `Origin-Agent-Cluster`,
  a restrictive `Permissions-Policy` (WebAuthn allowed for self); with TLS
  also HSTS (one year, no `includeSubDomains`: LAN names share parents) and
  `upgrade-insecure-requests`. Two profiles: **Strict** (Trusted Types
  enforced; forced for every non-loopback address) and **Compat** (the
  loopback default; Trusted Types report-only) — the reader still assigns
  `srcdoc`, a Trusted Types sink, until it moves to the sandbox origin or a
  `nail-reader` policy (WP45 / WP43).
* **Host / Origin** (`guard`): the allowlist now holds the loopback names
  plus `host_names`, the scheme (http / https) and the port (bare name on
  the scheme's default port). The proxy route checks `Host` only (sandboxed
  documents send `Origin: null` for fonts; the token is the capability).
* **Fetch metadata** (`guard`): requests with `Sec-Fetch-Site` other than
  `same-origin` / `none` are refused on the app origin unless they are a
  top-level `GET` navigation to a non-API path or a proxy request. This
  matters because the sandbox origin is *same-site* with the app (same
  host, other port): `SameSite=Strict` cookies alone would still flow on
  its requests.
* **Sandbox origin** (`sandbox`): the same host on its own port with its
  own guard and headers, serving only `GET /render/<render token>` (the
  message's `sanitized_html` from a `MailSource`, by default the service's
  `GetMessage`) and `GET /api/proxy/<token>`. Documents get `default-src
  'none'`, images / styles / fonts only from the sandbox origin (and
  `data:`), no `script-src`, `sandbox allow-popups
  allow-popups-to-escape-sandbox` (no scripts, no same-origin → an opaque
  origin whose requests carry no `SameSite` cookies), `frame-ancestors` =
  the app's origins, CORP `cross-origin` (the app is COEP), `no-store`.
  `nail-proxy:` URLs in the HTML become `/api/proxy/` on the sandbox
  origin. No WebSocket, blobs, login, bundle or test hooks there; cookies
  sent to it are never read.
* **Proxy route**: `GET /api/proxy/{token}` on both origins when a `Proxy`
  is configured; adds `Access-Control-Allow-Origin: *` (font loads are
  CORS requests from opaque origins; the bytes are public and the token is
  the capability) and never passes upstream headers such as `Set-Cookie`.

## Consequences

* WP45 wires: `ProxyKey::load_or_create` + `Signer` in the core, the
  `nail-mime` rewriting hook (`crates/nail-mime/src/remote_content.rs`, a
  WP42 slot this package could not edit in this stage) calling
  `Signer::rewriter`, `MessageBody.remote_content`, a `TargetCheck` backed
  by the allow-lists, render tokens handed to the UI with the sandbox
  origin, `Server::bind_with` from `nail serve` / the desktop's remote UI
  with `cert::load_or_create_self_signed`, `RemoteUiStatus.certificate`
  from `ServerIdentity::info`, `RegenerateRemoteCertificate` →
  `cert::regenerate_self_signed`, and the desktop `nail-proxy:` protocol
  → `Proxy::serve` + `RESPONSE_HEADERS`.
* Render tokens are bearer capabilities for one message's HTML for an hour;
  the core must hand them only to sessions allowed to read that message.
* Tested: 18 `nail-proxy` unit tests plus 8 integration tests (literal and
  resolved private addresses in IPv4 / IPv6 / mapped / NAT64 forms,
  redirects into private ranges, other ports, local names and schemes,
  DNS rebinding with a scripted resolver, header hygiene, limits, HTTPS
  with name verification, token forgery / purpose / expiry, the core
  check); 21 `nail-server` unit tests (suites, networks, certificate reuse /
  renewal / damage, ACME lock, headers, guard, sandbox CSP) and
  9 tests in `tests/transport.rs` over real sockets (TLS 1.3 and 1.2 with a pinned
  certificate, WebSocket over TLS, HSTS, header presence on every route
  and on refusals, sandbox isolation both ways, the proxy route). The
  existing `tests/server.rs` is unchanged and passes, and so does the web
  Playwright suite run against a `nail` built from this package.
