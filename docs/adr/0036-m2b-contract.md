# ADR 0036: M2b contract — remote UI, remote content, store v6, crate stubs, slots

Status: accepted · 2026-09-29 · M2b WP40

## Context

Wave M2b (plan section 7) hardens the remote UI and adds the installer and
CI: authentication and sessions (WP41), transport and browser hardening
with the remote-content proxy (WP42), the remote-UI and privacy UI (WP43)
and the installer / CI (WP44), built by four agents in parallel and
integrated by WP45. This ADR fixes the contract they build against, the
store schema they share and the slots that keep them apart.

Constraints (plan "Scope decisions for M2", design "Remote UI security"):
no GPL / LGPL / AGPL dependency; no OpenSSL build dependency unless an ADR
shows there is no alternative; internet mode is built but **locked** until
the penetration-test gate is recorded; the remote UI is off by default,
binds to loopback by default and needs TLS for any other address; no
default credentials.

## Decisions

### Protocol additions (`nail-api`, modules `remote` and `remote_content`)

Conventions of ADRs 0002 / 0012 hold; `PROTOCOL_VERSION` stays 1 (only
additions and one optional field). New ids: `RemoteUserId` (`ruser-<n>`),
`RemoteDeviceId` (`rdev-<n>`), `RemoteSessionId` (`rsess-<n>`, never the
token), `PasskeyId` (`passkey-<n>`), `PairingId` (`pair-<n>`, never the
code). New error codes with `error-*` messages: `forbidden` (grant lacks
the scope or `admin`; internet mode), `reauth_required`, `rate_limited`.

**Settings and status.** `RemoteUiSettings {enabled=false,
bind_address="127.0.0.1", port=4780, host_name?, tls: self-signed | acme |
off, acme {domains, contact_email, directory_url, accept_terms},
internet_mode=false, allowed_networks (CIDR), session_idle_minutes=30,
session_max_hours=12, reauth_minutes=5}` with `issues()` (shared
validation): `off` only on loopback (`tls-required`), internet mode refused
while `internet_mode_lock()` is `Some(pentest-pending)` — always in M2 —
(`forbidden`), ACME only in internet mode, CIDR / host-name / limit checks.
`host_name` exists because passkeys need a DNS name as relying-party id
(browsers refuse IP addresses). `RemoteUiStatus {state: stopped | starting
| running | failed, urls, certificate? {source, fingerprint_sha256,
subject_names, not_before, not_after}, internet_mode_locked,
internet_mode_lock_reason, active_sessions, error, message}`.

**Grants.** `RemoteGrant {mail: McpGrant, admin}` — a remote session has
the same scopes / accounts / folder exclusions as an MCP client (design:
"the same scopes as MCP clients") plus `admin` for administration, which
also needs a fresh re-authentication. `intersect` / `is_within` implement
"never more than granted": session ⊆ device ⊆ user. `RemoteAccessPreset`
(read-only, no-send, full) never includes `encrypted`, `context` or
`admin` (explicit extras).

**Read types.** `RemoteUser {id, name, display_name, created_at,
last_login_at, has_password, totp_enabled, passkeys[], grant, disabled,
locked_until}`, `Passkey`, `RemoteDevice {…, grant, revoked_at}`,
`RemoteSession {…, auth_method: passkey | password | password-totp |
pairing, expires_at, reauth_at, grant, ended_at, end_reason: logout |
revoked | expired | device-revoked | user-changed}`, `PairingCode {…,
state: pending | used | expired | cancelled, device_id}`,
`TotpEnrollment` (shown once), `RemoteAuditEntry {kind, outcome: success |
failure | denied, user, device, session, ip, user_agent, method, name,
error, detail}`, `RemoteAlert` (`new-device`, `lockout`). No read type
carries a secret.

| Request | Response | Served now |
| --- | --- | --- |
| `GetRemoteUiSettings` / `UpdateRemoteUiSettings {settings}` | `RemoteUi {settings, status}` | yes (validated; status `stopped` until WP45) |
| `RegenerateRemoteCertificate` | `RemoteUi` | `unsupported` (WP42 → WP45) |
| `ListRemoteUsers`, `CreateRemoteUser {name, display_name, grant}`, `UpdateRemoteUser {user_id, update}`, `DeleteRemoteUser`, `UnlockRemoteUser`, `RenamePasskey`, `DeletePasskey` | `RemoteUsers` / `RemoteUserSaved` / `Done` | yes |
| `SetRemotePassword {user_id, password?}`, `StartTotpEnrollment`, `ConfirmTotpEnrollment {code}`, `DisableTotp` | `RemoteUserSaved` / `TotpEnrollment` | `unsupported` (WP41 → WP45) |
| `ListRemoteDevices {user_id?}`, `UpdateRemoteDevice {name?, grant?}`, `RevokeRemoteDevice` | `RemoteDevices` / `RemoteDeviceSaved` / `Done` | yes |
| `ListRemoteSessions {filter}`, `RevokeRemoteSession`, `RevokeAllRemoteSessions {user_id?}` | `RemoteSessions` / `Done` | yes |
| `CreatePairingCode {user_id, device_name?, grant?, ttl_minutes?}` | `PairingCodeCreated {pairing, code, url, certificate_fingerprint}` | `unsupported` (WP41 → WP45) |
| `ListPairingCodes`, `CancelPairingCode` | `PairingCodes` / `Done` | yes |
| `ListRemoteAudit {user_id?, before?, limit}` | `RemoteAudit` | yes |
| `AllowRemoteContent {message_id, scope}` | `RemoteContentAllowed {entry}` | yes |
| `ListRemoteContentAllows`, `RemoveRemoteContentAllow {scope, value}` | `RemoteContentAllows` / `Done` | yes |

**Events:** `RemoteUiStatusChanged {status}`, `RemoteAccessChanged`
(users / devices / sessions / pairing codes), `RemoteSessionEnded
{session_id, reason}` (the browser holding it shows the login page; its
WebSocket is closed right after), `RemoteSecurityAlert {alert}`,
`RemoteContentPolicyChanged`.

**Login protocol.** `HttpOnly` cookies can only be set by HTTP responses,
so the browser logs in over one JSON route, `POST /api/auth`
(`AUTH_PATH`), with `AuthRequest` (tag `type`: `Status`, `Pair {code,
device_name}`, `Password {user_name, password, totp_code?, purpose: login
| reauth}`, `PasskeyStart {user_name?, purpose}`, `PasskeyFinish
{challenge_id, credential}`, `PasskeyRegisterStart {name}`,
`PasskeyRegisterFinish`, `Logout`) and `AuthResponse` (`Status {status:
AuthStatus}`, `LoggedIn {session}`, `Reauthenticated {reauth_until}`,
`TotpRequired`, `PasskeyChallenge {challenge_id, options}` (WebAuthn JSON
options), `PasskeyRegistered`, `LoggedOut`, `Failed {reason, retry_after_secs}`).
Failures are deliberately coarse (`invalid-credentials` for a wrong name
or password). Cookies: `__Host-nail-session` and `__Host-nail-device`
(`HttpOnly; Secure; SameSite=Strict; Path=/`); CSRF token from `Status`
in `x-nail-csrf`. This is the one route beyond the design's "three
routes" (bundle, WebSocket, blobs) besides the proxy route — a deliberate,
documented deviation.

**Remote content.** Blocked by default. `RemoteContentScope` = `message`
(message id), `sender` (lowercase address), `domain` (lowercase `From`
domain, exact; subdomains are separate entries — no public-suffix list
yet). `normalize_allow_value` / `sender_domain` are shared. The optional
`MessageBody.remote_content {resources, allowed_by?, sender?, domain?}`
tells the reader what to offer; when allowed, `sanitized_html` references
each remote resource as `nail-proxy:<token>` (`PROXY_URL_SCHEME`) — a
token the core signs over message id, original URL and expiry — and the
transport resolves it (web: `GET /api/proxy/<token>`, `PROXY_PATH`;
desktop: a custom protocol, WP45). The core checks the most specific entry
first (`Store::remote_content_allowed_by`).

### Store schema v6 (`nail-store`)

`remote_users`, `remote_devices`, `remote_passkeys`, `remote_sessions`,
`remote_pairing_codes`, `remote_throttle`, `remote_audit`,
`remote_content_allow`. **Hashed or referenced secrets only**: password
hashes must be Argon2id PHC strings (a `CHECK` and the accessor refuse
anything else), TOTP secrets are `keychain:` references (`CHECK`), device
secrets / session tokens / pairing codes are SHA-256 hex
(`hash_remote_secret`; the accessors refuse other shapes). Passkey rows
hold only the public credential. Invariants the accessors enforce (tested
in `tests/remote.rs`): grants narrow downwards (and narrowing a user or
device narrows what hangs below at once); a TOTP step is accepted once
(`accept_totp_step`); a pairing code is redeemed once before it expires
(`redeem_pairing_code`, one `NotFound` for every failure); a session is
found by its current token or, within the rotation grace period, the
previous one (`remote_session_by_token`), never when ended, expired, its
device revoked or its user disabled; disabling / deleting a user and
revoking a device end sessions. Throttle counters and lockouts are
persisted (a restart does not reset them). Settings live in `app_settings`
under `REMOTE_UI_SETTINGS_KEY`; the certificate record (certificate,
fingerprint, `keychain:` reference of the key) under
`REMOTE_UI_CERTIFICATE_KEY`, owned by WP42. **Parallel packages add no
migrations**; WP45 appends v7 if a package reports a need.

### Crates and libraries

Licences checked with `cargo metadata` (MIT / Apache-2.0 / BSD / ISC /
CC0 / Zlib / Unicode only). `cargo tree -i openssl-sys`, `-i aws-lc-rs` and
`-i aws-lc-sys` stay empty; everything TLS uses `ring`.

- **Passkeys: `webauthn_rp` 0.3 (MIT / Apache-2.0) instead of the plan's
  `webauthn-rs`.** `webauthn-rs-core` 0.5 / 0.6 depends on `openssl` and
  `openssl-sys` unconditionally, which the scope decisions rule out.
  `webauthn_rp` is a pure-Rust relying-party library on the RustCrypto
  generation we already build (p256 / p384 0.13, rsa 0.9, ed25519-dalek 2),
  with JSON (de)serialization of options and responses. Risk: a smaller
  project; WP41 wraps it behind its own module so it can be swapped.
- Passwords `argon2` 0.6 (Argon2id PHC; digest-0.11 generation, like the
  workspace `sha2`); TOTP `totp-rs` 6 (`std`, `otpauth`; no QR image
  feature — the UI renders the QR code); cookies `cookie` 0.18.
- Proxy: `hmac` 0.13 (+ workspace `sha2`), `ipnet` 2.12, the workspace
  `reqwest` / `hickory-resolver` / `url`.
- TLS: `rustls` 0.23 (ring, TLS 1.3 + 1.2), `tokio-rustls`, `rcgen` 0.14
  (ring), `instant-acme` 0.8 (`ring`, `hyper-rustls`; no aws-lc).
- Stub crates with library smoke tests on Windows MSVC:
  `nail-remote-auth` (Argon2id round trip into the store, RFC 6238 vector
  and `otpauth://` URL, WebAuthn creation / request options for a host
  name, cookie attributes), `nail-proxy` (HMAC sign / verify, private-range
  classification incl. IPv4-mapped IPv6, browser-style host parsing of
  `2130706433` / `0x7f.1`), `nail-server` (`tests/tls_libs.rs`: a
  `rcgen` self-signed certificate completes a TLS 1.3 handshake with a
  pinning client over `tokio-rustls`; `instant-acme` builds). Each works
  with `cargo test -p <crate>` alone.
- Expected shapes are in the crates' module docs. Neither new crate
  depends on `nail-core`; WP41 provides axum handlers / middleware and an
  exhaustive `required_access(&Request)`; WP42 provides signing and
  fetching for the core, `nail-server` and the desktop shell.

### Core

`Service::dispatch` routes every M2b request to `nail-core/src/remote.rs`
(one arm). It serves what needs only the store (table above), emits the
events, and refuses internet mode and invalid settings; everything needing
`nail-remote-auth` or `nail-server` answers `unsupported`. The UI mock
(`ui/src/features/remote/mock.ts`) behaves the same.

### Collision-free slots

| Area | Slot | Owner |
| --- | --- | --- |
| Authentication, sessions, pairing, rate limits, audit writes | `crates/nail-remote-auth/` | WP41 |
| Remote-content proxy | `crates/nail-proxy/`, `crates/nail-mime/src/remote_content.rs` (declared; the rewriting hook) | WP42 |
| TLS, certificates, ACME, headers (HSTS, CSP, Trusted Types, COOP/COEP, `frame-ancestors`), Origin/Host checks, sandbox origin, proxy route | `crates/nail-server/` | WP42 |
| UI | `ui/src/features/remote/` (`index.ts`, `mock.ts` answering `REMOTE_REQUESTS`, `messages.ts`), registered in `features/registry.ts`, state `data.features.remote` | WP43 |
| Widget kinds | `crates/nail-config/src/widgets/remote.rs` (prefix `remote-`) | WP43 |
| Strings | `locales/en/remote.ftl` + empty `locales/de/remote.ftl` | WP43 |
| Installer and CI | the `bundle` section of `app/src-tauri/tauri.conf.json`, `app/src-tauri/installer/`, `.github/workflows/`, `deny.toml` (repository root, new), `scripts/` | WP44 |

Integrator-owned during M2b (report, do not edit): `Cargo.toml` (the
dependencies above are pre-declared — ask before adding more),
`Cargo.lock` beyond a package's own new dependency, `nail-api`,
`nail-store` migrations and accessors' signatures, `nail-core`,
`nail-cli`, `app/src-tauri/src`, `nail-config` outside the widget slot,
`App.tsx`, `src/api/mock.ts`, `src/state/uiStore.ts`, `defaults/*.kdl`,
`e2e/`. WP42 may change `nail-server`'s public API as long as `nail-cli`
and the desktop shell keep compiling (it owns the crate; WP45 wires the
rest).

## Consequences

- WP41–WP44 compile against fixed types and generated TypeScript; the
  suites stay green in between.
- WP45 wires: `nail-remote-auth` into `nail-server` and the admin requests
  (password, TOTP, pairing codes), TLS / certificates into `nail serve` and
  the desktop app, `RegenerateRemoteCertificate`, the remote-UI status,
  `MessageBody.remote_content` and proxy URLs (core → `nail-mime` hook →
  `nail-proxy`), the desktop proxy protocol, audit entries for admin
  changes, stale message allow-list cleanup, and the M2b Playwright
  scenarios.
- The WebAuthn library differs from the plan; recorded here, revisited if
  `webauthn-rs` drops its OpenSSL dependency.
- Test fix outside the contract: `nail-core/tests/security.rs`
  `search_count` treats `unavailable` (the vault migration swapping the
  index) as "not yet" inside `eventually`, which made
  `encryption_at_rest_on_and_off_keeps_the_data` flaky (about 1 run in 5).
