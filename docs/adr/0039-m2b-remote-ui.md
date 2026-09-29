# ADR 0039: Remote-UI and privacy UI

Status: accepted · 2026-09-29 · M2b WP43

## Context

WP43 builds the UI for remote access and remote content against the M2b
contract (ADR 0036): the browser login (passkey first, password + TOTP),
pairing on both sides, users / devices / sessions, the remote-UI
settings with internet mode locked, the reader's "load remote content"
bar, the allow-lists, the security audit view and security alerts. The
core serves password, TOTP, pairing-code creation and certificate
regeneration only after WP45, so the UI is built and tested against a
mock that behaves like the finished core.

## Decisions

### Everything lives in `ui/src/features/remote/`

| File | What |
| --- | --- |
| `Login.tsx` | `RemoteLogin` (pairing → optional passkey registration; passkey first, password, TOTP step, throttle countdown, unreachable/retry) and `RemoteReauth` (purpose `reauth`) |
| `authClient.ts` | `AuthTransport`; `HttpAuthTransport` = `POST /api/auth` with the `x-nail-csrf` token from `Status` (fetched on demand, dropped after `LoggedIn` / `LoggedOut` so a rotated token is picked up) |
| `passkeys.ts` | `PasskeyApi` (injected for tests and Playwright's virtual authenticator); `browserPasskeys` wraps `@simplewebauthn/browser`; `passkeyProblem` maps a closed prompt to `cancelled` (a status line, not an alarm) |
| `authConfig.ts` | the transport / passkeys the `remote.logout` action uses (`configureRemoteAuth`) |
| `RemoteSettings.tsx` | widget `remote-settings` |
| `RemoteUsers.tsx` | widget `remote-users` (users, access preset + admin, password, TOTP with QR code, passkeys, lockout, disable/delete) |
| `PairDevice.tsx` | dialog `remote-pair` (desktop side of pairing) |
| `RemoteDevices.tsx` | widget `remote-devices` (devices, sessions, pending codes) |
| `RemoteAudit.tsx` | widget `remote-audit` |
| `RemoteContent.tsx` | `RemoteContentBar`, widgets `remote-content` and `remote-content-allows` |
| `Alerts.tsx` | `RemoteAlerts` feature effect (toasts for `RemoteSecurityAlert`) |
| `mock.ts`, `mockAuth.ts` | the mock core (all 27 requests) and `MockAuthServer` (the login route) on shared state |

### The login page is outside `NailClient`

It runs before a session exists (ADR 0036), so it takes an
`AuthTransport`. The page needs only the I18n provider (and the theme for
styling). The app shell decides when to show it (WP45).

### Passkeys: `@simplewebauthn/browser` (MIT)

Maintained, zero dependencies, handles the base64url ⇄ `ArrayBuffer`
conversion of the WebAuthn JSON options/credentials, and classifies
ceremony errors. The options from `webauthn_rp` (WP41) are
`PublicKeyCredential{Request,Creation}OptionsJSON`; a `{ publicKey: … }`
wrapper is accepted too.

### QR codes: `uqr` (MIT)

The ADR 0036 TOTP enrollment leaves the QR code to the UI. `uqr` returns
the module matrix only; the UI draws one SVG path (`remote-qr.modules`)
and one background rect (`remote-qr.background`) so the theme colours
them (`fill`). Also used for the pairing URL.

### Settings are checked in the UI too

`settingsIssues` mirrors `RemoteUiSettings::issues` (internet mode
locked), so the form names the problem before saving and disables
"Save"; the core stays authoritative and its error message's issue name
is shown as well. Internet mode is a disabled checkbox whose accessible
description is the lock reason (`remote-internet-lock-<reason>`). TLS
choices are self-signed and off; ACME is not offered outside internet
mode.

### Presets

`presetGrant` mirrors `RemoteAccessPreset::grant` (read-only / no-send /
full, all accounts); `admin` and the extra scopes `encrypted` /
`context` are separate choices, kept when the preset changes. A grant
narrowed to accounts or folders shows as "custom" and is saved as it is.

### Remote content: "once" is the per-message allow

The contract has three scopes: `message`, `sender`, `domain`. The bar's
first choice, "Load for this message", is the `message` scope (it stays
allowed for that message, which is what users expect when they reopen
it). The bar reads `MessageBody.remote_content` (count, `allowed_by`,
sender, domain); with a core that only sets `remote_content_blocked` it
falls back to the `From` address. When loaded it says why and offers to
block again (`RemoveRemoteContentAllow` with the entry's value). The
`remote.content.allow ["sender" | "domain"]` action makes this
keyboard-bindable.

### Alerts stay until acknowledged

`RemoteSecurityAlert` (new device, lockout) pushes a toast into the
feature's store (max 5); toasts are `role="alert"`, offer "Review
devices" (opens the `settings` window) and "Dismiss"; Escape dismisses.
No auto-dismiss: these matter.

### Mock behaviour (for UI work and tests)

The mock implements what WP41 / WP42 / WP45 will: password (min. 12
characters, like `PasswordPolicy`), TOTP enrollment (accepts
`MOCK_TOTP_CODE` = `123456`), pairing codes (`XXXX-XXXX` from WP41's
alphabet, single use, expiry, cancel), a `running` status with URLs and
a self-signed certificate while enabled, certificate regeneration,
audit entries, lockout after 5 failures (per address and per name, with
a `lockout` alert), a `new-device` alert on pairing, and
`decorateRemoteContent` for `MessageBody.remote_content`. Secrets are
plain text in the mock only (documented). `unsupported: [...]` switches
requests back to `unsupported` to test the pre-WP45 behaviour.

## Consequences

- WP45 must: register the widget kinds in
  `crates/nail-config/src/widgets/remote.rs`, place the widgets / dialog
  / actions in `defaults/layout.kdl`, add theme parts, chain
  `decorateRemoteContent` in `MockTransport`'s `GetMessage`, mount
  `RemoteLogin` in web mode (and set `remoteStore.setSession` /
  `configureRemoteAuth`), replace the reader's generic
  `message-view.remote-content-banner` with `RemoteContentBar` (or drop
  it in favour of the `remote-content` widget), and add the Playwright
  scenarios of the plan.
- New npm dependencies: `@simplewebauthn/browser` 14 (MIT, no deps),
  `uqr` 0.1 (MIT, no deps).
