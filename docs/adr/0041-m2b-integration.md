# ADR 0041: M2b integration — remote UI with login, remote content through the proxy, launch requests

Status: accepted · 2026-09-29 · M2b WP45

## Context

Wave M2b built four packages in parallel against the contract of ADR 0036:
authentication and sessions (`nail-remote-auth`, ADR 0037), transport and
browser hardening with the remote-content proxy (`nail-server`,
`nail-proxy`, ADR 0038), the remote-UI and privacy UI
(`ui/src/features/remote`, ADR 0039) and the installer / CI (ADR 0040).
WP45 wires them into `nail serve`, the desktop app and the core, and adds
the Playwright scenarios of plan section 7.

## Decisions

### Who owns the listener

`nail-server` depends on `nail-core`, so the core cannot start servers. The
core keeps the remote-UI **state** (`remote.rs`: settings, the reported
status, the sandbox origin) and a command channel; a host attaches with
`Service::attach_remote_ui()` and receives `RemoteUiCommand`s
(`Apply(settings)`, `RegenerateCertificate(reply)`), reporting back with
`Service::report_remote_ui(status, sandbox_origin)`.

`nail_server::RemoteUiHost` is that host (`remote_ui.rs`). It applies the
stored settings at start and every change: stop what runs, then (if
enabled) load or create the self-signed certificate (`cert.rs`, kept across
restarts), bind `Server::bind_with` with TLS, the host names (the
configured one or the computer's name; the LAN address when bound to
`0.0.0.0` / `::`), the allowed networks, the proxy, a sandbox origin, the
**strict** header profile and **authentication**
(`TransportOptions::auth`). It sets `RemoteAuth`'s config (relying party
`RelyingParty::for_port(host, port)` with TLS, the origins, the certificate
fingerprint) and runs the session sweeper while serving. A failure (port
busy) is reported as `failed` with the error. Internet mode and ACME stay
refused. Without a host (core tests, embedders) settings are stored and
the status stays `stopped`; `RegenerateRemoteCertificate` answers
`unsupported`.

`nail serve` and the desktop app both start a `RemoteUiHost`; the remote UI
is **off by default**. The desktop's own UI (the loopback `nail serve` in
the browser, the Tauri window) is unchanged: no login, loopback only. That
keeps the e2e suite's local mode working. The desktop app serves the
remote UI's files from the bundled `ui/` resource (`tauri.conf.json`
`bundle.resources`), `NAIL_UI_DIR`, or the repository's `ui/dist`.

### Authentication on the wire

With `auth` set, the server merges `nail_remote_auth::http::router`
(`POST /api/auth`) and wraps the WebSocket, blob and proxy routes in
`require_session`. A middleware copies the listener's
`ConnectInfo<ClientAddr>` into `ConnectInfo<SocketAddr>`, which
`nail-remote-auth` reads for per-address rate limits and the audit log.
The WebSocket of a remote session sends every request through
`Service::handle_remote(ctx, request)` and re-reads the session on each
request (a re-authentication or a narrowed grant applies at once) and
every 60 seconds; it closes when the session ends (`RemoteSessionEnded`
for it, or `refresh` returning nothing). An open socket counts as
activity (the idle limit ends abandoned sessions; the absolute limit
always applies). Blobs need `raw-source` and a grant for all accounts.

### Requests from remote sessions (`nail-core/src/remote_access.rs`)

1. `RemoteAuth::authorize` (the exhaustive `required_access`; denials
   audited).
2. The grant's accounts and folder exclusions, resolved like an MCP
   client's (`nail_mcp::access::Access`): accounts, folders and messages a
   request names must be granted (`forbidden` otherwise); list answers
   (accounts, folders, messages, threads, search hits, account statuses)
   are narrowed. Only when the grant is narrowed; the full grant skips it.
3. The `encrypted` gate: `GetMessage` of an end-to-end encrypted message
   needs the `encrypted` scope (`Service::message_is_encrypted`, like MCP).
4. `SetRemotePassword`, TOTP and `CreatePairingCode` run with the session
   as the actor (`RemoteAuth::handle_admin(request, Some(ctx))`); from the
   desktop with `None`.

Events are forwarded to remote sockets unfiltered (they carry ids and
counts, not content); narrowing events per grant is a follow-up.

### Remote content

- **nail-mime** (`remote_content.rs`, `sanitize.rs`): the sanitizer counts
  every remote reference (`src`, `background`, CSS `url()`), and
  `parse_message_with(raw, Some(rewriter))` replaces each with the
  rewriter's `nail-proxy:<token>` (removing what it refuses). The rewriter
  is a closure (`ProxyRewriter`), so nail-mime has no keys and no network.
  Rewritten CSS URLs are quoted and refused if the replacement could
  escape the string.
- **Core**: `RemoteState` holds `RemoteAuth`, the proxy `Signer` (key in the
  keychain, `keychain:nail/proxy/signing-key`) and the `Proxy` (fetcher +
  a `TargetCheck` that re-reads the allow-lists, so removing an entry stops
  tokens already handed out). Both are built **lazily** — the key is read
  or created only when content is first allowed or served — so tests that
  open a service with the OS keychain never touch it. `GetMessage` fills
  `MessageBody.remote_content` (resources, `allowed_by`, sender, domain);
  for an allowed message it rewrites the HTML, and `remote_content_blocked`
  is `false` (references the proxy refuses, such as private literals, stay
  removed for good). `purge_history` also removes remote-UI audit entries
  older than 90 days and per-message allows of deleted messages
  (`Store::purge_stale_remote_content_allows`, new accessor, no migration).
- **Proxy endpoints**: the loopback `nail serve` mounts `/api/proxy/`
  (`bind_with` + `proxy`), the remote UI mounts it behind the session
  middleware, the sandbox origin serves it by token only, and the desktop
  app registers the `nail-proxy` protocol (`http://nail-proxy.localhost/…`
  on WebView2) answered by `Proxy::serve` with the inert response headers.
- **Reader**: `readerDocument(html, css, proxyBase())` points
  `nail-proxy:` at the transport (`<origin>/api/proxy/` or the desktop
  protocol) and adds exactly that origin to the document's `img-src` /
  `font-src`. The Tauri CSP allows `http://nail-proxy.localhost` for
  images and fonts.
- **Seeded runs never reach the internet**: `demo_options` sets a fetcher
  whose resolver knows no names; demo-multi's resolves
  `images.nail.example` to its mock HTTP server (loopback, allowed for that
  one port through `nail-proxy`'s `test-support` hook, which the `demo`
  feature of `nail-core` enables) and `private.nail.example` to `10.0.0.7`
  (refused). Test control `remote-content` gives the base URLs and the
  paths the image host was asked for.

### Mail HTML on the sandbox origin; Trusted Types

The remote UI enforces Trusted Types. The reader and the composer preview
set `srcdoc` through the `nail-reader` policy (`theme/trustedHtml.ts`)
instead of React's `srcDoc` prop (a plain string would throw). For remote
sessions the core adds `MessageBody.render_url` (new optional field):
`<sandbox origin>/render/<render token>`
(`nail_api::remote_content::RENDER_PATH`); the reader frames that URL,
passing the reader stylesheet as the `css` query parameter (the sandbox
escapes `</` and limits it to 16 KiB). So in the remote UI mail HTML runs
on a separate origin, as the design asks; the desktop keeps `srcdoc`. The
sandbox origin uses the primary host name, so the browser's certificate
decision covers it. CSP source lists leave out IPv6 literals (browsers
reject `https://[::1]:…`).

### Browser shell

`App.tsx` asks `POST /api/auth` for the status before opening the
WebSocket (`RemoteGate.tsx`): no route (404) = the local UI, open; a valid
session = open; otherwise `RemoteLogin`. When this browser's session ends,
the client closes and the login page returns. `ReauthTransport` wraps the
WebSocket transport: a `reauth_required` answer shows `RemoteReauth` and
the request is sent once more after it (cancel keeps the error). The login
page runs before the theme is loaded, so it is unstyled (follow-up).

### Passkeys: credProtect not enforced by the browser

`webauthn_rp`'s passkey defaults ask the browser to enforce
`credProtect = userVerificationRequired`. Browsers then refuse to create a
passkey on authenticators without the extension (Chrome's virtual
authenticator, and many platform authenticators). The extension is now
requested but not enforced (`ExtensionInfo::AllowDontEnforceValue`);
every login still requires user verification, which is what the policy
protects.

### Launch requests (`nail_api::launch`)

`LaunchRequest` / `MailtoLink` moved from the shell into the contract
(ts-rs; serde tagged by `type`, snake_case fields). The shell parses its
command line (`launch::parse`, `from_args`) and calls
`Service::push_launch_requests`; `Event::LaunchRequested` tells the UI,
which takes them with `TakeLaunchRequests` (each once) in the new `launch`
feature: `Compose` opens the composer with the link's fields (plain text;
the plain M1a composer when the core has no rich one), `OpenMessageFile`
imports the file with `ImportMessageFile` into the local "Opened files"
account and selects it, calendars and contacts show "not supported yet"
(M3). Both requests are desktop-only in `required_access`. A second start
of the app forwards its targets to the running one
(`tauri-plugin-single-instance` 2.5.1, MIT / Apache-2.0; not for seeded
runs, which are independent). `nail-cli` defaults to `<root>\data` and
`<root>\config` in the portable zip (`bin\nail.exe` beside the `portable`
marker's directory).

## Tests

- Rust: `nail-mime` rewriting (`remote_content.rs`), `nail-store` stale
  allows, `nail-core` `tests/remote.rs` (remote content blocked → allowed →
  proxied, removal stops tokens, admin requests, remote authorization and
  narrowing, the host link, proxy key persistence), `tests/launch.rs`,
  `tests/restart.rs` (the M2a carry-over: an encrypted profile reads again
  after a restart with the same keychain; not without it), `nail-server`
  `tests/remote_ui.rs` (TLS listener with login from the settings, pairing
  answer with URL and fingerprint, certificate regeneration, disabling
  closes the port, a busy port is `failed`), the app's proxy protocol.
- UI: login gate, re-authentication retry, launch handling, Trusted Types
  and the reader's proxy / sandbox handling.
- Playwright: `remote.spec.ts` (pairing a second browser with a code from
  the desktop dialog, passkey login with the virtual authenticator,
  password + TOTP, password guessing slowed down, session revoke logs the
  browser out, internet mode cannot be enabled, CSP blocks an injected
  inline script locally and Trusted Types block it in the remote UI),
  `remote-content.spec.ts` (blocked → allowed per sender → loaded through
  the proxy; private target 403, forged token 404, loopback literal never
  signed), `desktop.spec.ts` (a `mailto:` start opens the composer). The
  remote tests trust the self-signed certificate by key
  (`--ignore-certificate-errors-spki-list`) where WebAuthn needs a page
  without certificate errors.

## Consequences and follow-ups

- The installer smoke test runs in CI (`installer` job); it was run by
  WP44 locally and is not repeated by WP45.
- Follow-ups: themed login page (serve the default theme before login);
  per-grant event filtering on remote sockets; message-bound render tokens
  (bind to the session); a `once` remote-content scope; per-session proxy
  rate limits; ACME end to end once internet mode unlocks; the throttle
  window measured from the last failure (store v7); `in-reply-to` of
  `mailto:` links is not used yet.
