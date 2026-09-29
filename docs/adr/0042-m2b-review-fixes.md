# ADR 0042: M2b review fixes: exhaustive remote request checks, encrypted gate on drafts and search, event filtering, LAN-only peers, portable UI files, pairing pause

Status: accepted · 2026-09-29 · M2b review

## Context

The M2b reviews found that remote-UI sessions could get more than an MCP
client with the same scopes, which ADR 0041 promised they never would:
`check_targets` listed only some requests and let everything else
through, so `NewDraft` with a forward context returned the decrypted body
(and copied the attachments) of any message, drafts, the outbox, rule dry
runs and remote-content allow labels of every account were readable, and
drafts could be saved and sent as another account's identity. The remote
`Search` was an oracle over decrypted mail when decrypted indexing was on.
Every core event went to every remote socket, including security alerts
with user names and client addresses. "LAN" mode accepted any peer that
reached the port. The portable zip had no UI files, so its remote UI
answered every page with 404, and the installed CLI looked for them in the
wrong place. A trickle of wrong pairing codes could keep pairing paused.
This ADR records the fixes; ADR 0041 points here.

## Decisions

### Every remote request is checked (`nail-core/src/remote_access.rs`)

- `Service::check_remote_request` is an exhaustive match over `Request`
  (no default arm), run for every remote request, not only for narrowed
  grants (the `encrypted` gate applies to full grants too).
  - Administration, self-service and desktop-only requests rely on
    `authorize`; the arm re-checks with `required_access` that they really
    are `Admin`, `AdminOrOwn` or `DesktopOnly` and refuses otherwise, so a
    later change of their access class cannot open a hole.
  - Accounts, folders (a real folder's account, a virtual folder's
    virtual account, exclusions), messages, identities (a draft belongs to
    its identity's account, like MCP), stored drafts, outbox entries and
    queued operations a request names must be granted.
  - The message a draft quotes, forwards or redirects (`context`,
    `AutomaticMail::Redirect`) must be readable: granted and, if it is
    end-to-end encrypted, the grant has `encrypted`. This covers
    `NewDraft`, `SaveDraft`, `ConvertDraft`, `GetDraft`, `SendDraft` and
    `DeleteDraft`.
  - Rules span every account: `ListRules`, `SaveRule`, `DeleteRule`,
    `ReorderRules`, `ListRuleRuns`, `UndoRuleRun` and runs or dry runs
    without messages / a folder need a grant for all accounts without
    excluded folders (like MCP's rules resource); runs on named messages
    or one folder check those.
- Answers are narrowed (`narrow_remote_response`), for every grant:
  identities, the chosen identity, virtual accounts, drafts (by
  `draft_visible`: identity account and readable original; a draft whose
  original is gone needs a full grant with `encrypted`), the outbox, rule
  dry-run matches, per-message remote-content allows (their labels are
  subjects) and the UI context (like MCP's `get_ui_context`: the selected
  text only with a readable open message). A `Draft` answer the grant does
  not cover (e.g. `NewDraft` picking another account's default identity)
  becomes `forbidden`; a thread with no granted message too.
- `SuggestAddresses` answers an empty list for narrowed grants: collected
  addresses are not stored per account.

### Remote search (`remote_search`)

Like MCP's restricted search: when the grant is narrowed, or the search
index holds decrypted text (`index_decrypted` non-empty) and the grant
lacks `encrypted`, hits are filtered *before* paging (at most 5000 scanned,
200 per page) and encrypted messages are dropped. `total` then counts the
covered hits found (at least one more than the page when more exist); no
unfiltered count leaks. Full grants with nothing to hide use the plain
search.

### Events on remote sockets (`Service::remote_event`, `nail-server` `ws.rs`)

The socket forwarder passes each event through `remote_event` with the
session's current grant:

- admin-only: `RemoteSecurityAlert`, `RemoteUiStatusChanged`,
  `RemoteAccessChanged`, `McpContextAccess`, `McpClientsChanged`;
- by account / folder: `NewMail`, `MailChanged` (the folder list is
  narrowed; dropped when empty), `SyncProgress`, `SyncFailed`,
  `AccountStatus`, `OpStatus`, `SendStatus` (the outbox entry's account;
  an entry already gone only for all-account grants);
- `RuleRunFinished` only for full grants (rules span every account);
- `RemoteSessionEnded` only for the session's own end (or admins);
- never: `OAuthFinished`, `LaunchRequested` (desktop flows);
- everything else (config, keys, settings changed, vault progress, rules
  changed) as before: no data beyond a signal to refetch.

The session's own `RemoteSessionEnded` is detected before filtering, so
the socket still closes.

### LAN mode accepts local peers only (`nail-api`, `nail-server`)

- `nail_api::remote::is_local_address` / `is_local_network`: loopback,
  RFC 1918, link-local (169.254/16, fe80::/10), CGNAT (100.64/10, used by
  Tailscale) and IPv6 unique local (fc00::/7); IPv4-mapped IPv6 counts as
  IPv4.
- `TransportOptions::public_peers` (default `false`) and
  `tls::PeerPolicy`: outside internet mode a peer must be loopback or
  local *and* within `allowed_networks` if any. An empty list means "the
  local networks", no longer "everyone who reaches the port". The
  remote-UI host always passes `false` (internet mode is refused).
- `RemoteUiSettings::issues` adds `public-network` (answered `forbidden`)
  for an allowed network that is not entirely local while internet mode is
  off or locked. The UI mirrors it (`isLocalNetwork`, Fluent key
  `remote-issue-public-network`).

### UI files in the portable zip and for the installed CLI

- `portableLayout` (scripts/lib/build.mjs) packs `ui/dist` as `Nail/ui/`
  (the installer's `bundle.resources` layout) and refuses a build without
  `index.html`; `listFiles` walks the directory. The installer smoke test
  checks `ui\index.html`.
- `nail serve` (`resolve_ui_dir`) also tries `ui` in the executable's
  parent directory (`<Nail>\bin\nail.exe` → `<Nail>\ui`); the desktop's
  `remote_ui_dir` also tries `ui` beside the executable. The build
  machine's `ui/dist` stays the last fallback.

### Pairing pause (`nail-remote-auth` throttle)

The global pairing key gets its own one-hour window
(`ThrottlePolicy::global_window_secs`) instead of the week of address and
account keys, and a successful pairing clears it along with the address
key. Pausing pairing now takes 100 failures within an hour from addresses
that each hit their own backoff and lockout; a trickle of one wrong code a
minute no longer keeps it paused.

## Consequences

- Remote sessions with a grant for some accounts cannot use rules and get
  no address suggestions; search pages for them are exact but `total` is
  a lower bound.
- Drafts replying to encrypted mail are invisible to remote sessions
  without `encrypted`, including their deletion.
- A remote UI behind a reverse proxy on the same machine still works
  (loopback peers); access from outside the local networks needs internet
  mode, which stays locked until the M2 security gate.
