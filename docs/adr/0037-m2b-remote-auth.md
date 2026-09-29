# ADR 0037: Remote-UI authentication and sessions

Status: accepted · 2026-09-29 · M2b WP41

## Context

Plan M2b WP41 and design "Remote UI security": passkeys first, password +
TOTP as the fallback, one-time pairing codes approved on the desktop,
short-lived device-bound sessions in hardened cookies with rotation,
re-authentication for sensitive actions, per-address and per-account rate
limits with exponential backoff and lockout notifications, session list and
revoke, a new-device alert and an audit log. The contract (ADR 0036) fixed
the protocol types, the store schema v6 (hashed / keychain-referenced
secrets only) and the slot `crates/nail-remote-auth`. `nail-server`
mounts the axum pieces and the core calls the admin operations (WP45).

## Decisions

### Shape

`nail_remote_auth::RemoteAuth` (cheap to clone) holds the `Store`, the
keychain (`SecretStore`), a `Clock` (tests use `ManualClock` to move time
and simulate clock skew), an `EventSink` (the core's broadcast), an
`AuthConfig` (relying party, allowed origins, certificate fingerprint;
replaceable at run time) and an `AuthPolicy` (limits; the defaults are the
production values). Remote-UI settings (idle / maximum session age,
re-authentication window) are read from the store on every use, so desktop
changes apply at once. The crate does not depend on `nail-core`.

| Piece | For |
| --- | --- |
| `http::router(auth)` | `POST /api/auth` (login protocol) |
| `http::require_session` | middleware for the WebSocket upgrade, blobs, proxy |
| `RemoteSessionCtx` | extractor (session, device, client, `reauth_until`) |
| `RemoteAuth::authorize(ctx, &Request)` | per-request check + audit (WebSocket frames) |
| `RemoteAuth::refresh(ctx)` | re-validate a long-lived WebSocket session |
| `RemoteAuth::handle_admin(Request, actor)` | `SetRemotePassword`, TOTP, `CreatePairingCode` |
| `RemoteAuth::sweep` / `spawn_sweeper` | idle expiry + ceremony cleanup |

### Credentials

- **Passwords:** Argon2id PHC strings, OWASP parameters (19 MiB, t = 2,
  p = 1), at least 12 characters, at most 1 KiB, no control characters.
  Hashing and verification run on a blocking thread. An unknown user or a
  user without a password is verified against a dummy hash of the same
  cost, so timing does not reveal whether a name exists.
- **TOTP:** RFC 6238 SHA-1 / 6 digits / 30 s, ±1 step skew, codes
  compared in constant time (`totp-rs`), each step accepted once
  (`Store::accept_totp_step`, which also refuses steps older than the last
  accepted one). Secrets live in the keychain
  (`keychain:nail/remote-totp/<user>`); enrollment keeps the new secret
  under `…/remote-totp-pending/<user>` until a code confirms it, and the
  confirming step is consumed so it cannot log in.
- **Passkeys:** `webauthn_rp` 0.3 (ADR 0036) with its relaxed JSON
  parsers (feature `serde_relaxed`) for what browsers really send.
  Relying-party id = the configured host name; **IP addresses are
  refused** (`passkey::check_rp_host`), as are numeric last labels.
  Discoverable credentials, user verification required, `credProtect`
  "user verification required", `excludeCredentials` on registration,
  origins checked against `AuthConfig.origins`, signature counter must
  grow (a cloned authenticator fails). The stored credential is JSON with
  only public data (user handle, transports, the library's encoded static
  and dynamic state). All passkeys of a user share one random 64-byte
  user handle. Ceremony state never leaves memory: a random challenge id
  maps to the library's server state, single use, 5 minutes, at most 256
  open (then `rate-limited`). A ceremony is bound to the device (login) or
  session (re-authentication, registration) that started it.

### Pairing

Codes are `XXXX-XXXX` over 29 symbols without look-alikes (`0/O`, `1/I/L`,
`U/V`), ≈ 38.9 bits; normalized (case, spaces, dashes) and stored as
SHA-256; 10 minutes by default, 1–60 allowed; single use
(`Store::redeem_pairing_code`). Creating one on the desktop is the
approval; the pending pairing carries the device name and grant (narrowed
to the user's). Redeeming creates the device (random 256-bit secret in the
device cookie, stored hashed) and a `pairing` session, raises
`RemoteSecurityAlert { NewDevice }` and is audited. The device name is
validated **before** the code is spent.

### Sessions and cookies

- `__Host-nail-session` / `__Host-nail-device`: `HttpOnly; Secure;
  SameSite=Strict; Path=/`, no `Domain`. A cookie sent twice counts as
  absent (shadowing attempt).
- A session is valid only with the device cookie of its own device
  (stolen session cookie alone, or mixed with another device's cookie, is
  refused), within the idle limit and the absolute limit (both from the
  settings, also re-checked against `created_at` in case the maximum was
  lowered).
- Tokens rotate every 5 minutes of use; the previous token stays valid for
  60 s (requests in flight). The rotation is claimed in memory before the
  store is updated, so concurrent requests do not rotate twice.
- Logging in or pairing again ends the browser's previous session.
  Credential changes (`SetRemotePassword`, `DisableTotp`) end the user's
  other sessions (`user-changed`), keeping the actor's own.
- `last_seen_at` is written at most every 15 s.

### CSRF and origin

- The login route requires `Content-Type: application/json` (a
  cross-site form cannot send it without a preflight), an `Origin` in
  `AuthConfig.origins` (or, when none are configured, equal to `Host`),
  and — for every step but `Status` — the `x-nail-csrf` header.
- CSRF tokens are `HMAC-SHA-256(process key, binding)`, compared in
  constant time (`subtle`). The binding is the paired device (or
  "anonymous" before pairing), so it survives login, logout and token
  rotation; a pairing changes it, and the pairing response carries the new
  token in the `x-nail-csrf` **response header**. A restart invalidates
  tokens (the browser asks `Status` again after a `bad-request`).
- `require_session` checks the header on every non-GET/HEAD/OPTIONS
  request.

### Brute force

Counters per `ip:<address>` (IPv6 per /64, IPv4-mapped as IPv4),
`user:<lowercase name>` (the key the core clears on `UnlockRemoteUser`)
and `global:pairing`, persisted in `remote_throttle`:

- 3 free failures, then a delay of 1, 2, 4 … s (at most 5 min) before the
  next attempt is evaluated; earlier attempts get `rate-limited` with
  `retry_after_secs` and are not evaluated.
- Lockout at 10 failures per account / 30 per address: 15 minutes,
  doubling with every further failure up to 24 hours; the account's
  `locked_until` is set (the desktop shows it), `RemoteSecurityAlert {
  Lockout }` is raised and audited once per lockout.
- The window is **7 days**: with a short window, waiting out the backoff
  also aged the count out and the lockout was never reached (found by the
  brute-force test). A success clears the address's and account's counts.
- Pairing failures count per address and globally; the global key only
  pauses pairing for 60 s at 100 failures (no backoff, no escalation), so
  an attacker cannot shut pairing down for long.

### Authorization

`required_access(&Request)` is an exhaustive match (a new request does not
compile until it is classified) returning the wire name and one of:
`Session`, `Scope(McpScope)`, `Sensitive(scope)` (scope + fresh
authentication: `UnlockKey`), `Admin` (grant `admin` + fresh
authentication: settings, accounts, keys, MCP clients, customization,
remote-UI administration), `AdminOrOwn` (self-service without
re-authentication: own sessions/devices list, revoke own session/device)
and `DesktopOnly` (`StartOAuth`, `CancelOAuth`, `OpenConfigFile`,
`ReportUiContext`). "Fresh" = login or re-authentication within
`reauth_minutes`. `authorize` audits every denial and every allowed
non-read request. The grant's account list and folder exclusions are
enforced by the core when answering (helpers `account_allowed`,
`folder_allowed`), as for MCP.

### Audit

Login / re-authentication (success, failure with the tried name and a
detail), pairing (success, failure), pairing-code creation, lockout,
logout, passkey registration, password and TOTP changes, and remote
requests (denied always, allowed writes). Audit write failures are logged,
never fatal.

### Libraries

Everything from ADR 0036's workspace set, with features `serde_relaxed`
(webauthn_rp), `zeroize` (argon2, totp-rs). New: `subtle` 2.6
(BSD-3-Clause) for the CSRF comparison; `chrono` / `hmac` (workspace);
dev-only `ed25519-dalek` 2.2 (BSD-3-Clause) for the software
authenticator. All were already in the lock; no OpenSSL, no aws-lc.

## Consequences

- Tests (94, `cargo test -p nail-remote-auth`) cover brute force on
  passwords and pairing codes (backoff, lockout, alerts, escalation),
  replay of pairing codes, TOTP codes and WebAuthn challenges / assertions,
  cloned authenticators, phishing origins, cookie tampering (missing,
  forged, altered, swapped device cookies, duplicate cookies), rotation
  with grace, idle / absolute expiry, TOTP clock skew (±1 step accepted,
  ±2 refused), CSRF / origin / content-type on the HTTP route, and that
  every `Request` variant has an arm with its wire name.
- WP43's UI must: call `Status` on load and after a `bad-request`; take
  the new CSRF token from the pairing response header; handle
  `rate-limited` / `locked-out` with `retry_after_secs`.
- WP45 wires it (see the package report): router + middleware in
  `nail-server` with `ConnectInfo`, `authorize` per WebSocket request,
  `refresh` + `RemoteSessionEnded` to close sockets, `handle_admin` in
  the core, `AuthConfig` from the settings / certificate, the sweeper.
- Not done: account-scoped filtering inside the core's answers (WP45),
  WebAuthn attestation other than `none`/self, passkey login by user name
  with non-discoverable credentials (discoverable only).
