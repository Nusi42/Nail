# ADR 0075: Exchange common layer — Autodiscover with engine choice, Windows sign-in (SSPI), Exchange ↔ Nail mapping, EWS retirement

Status: accepted · 2026-10-01 · M3c WP93

## Context

Plan section 8, WP93: "Mapping between Exchange concepts and Nail's
(categories, Focused Inbox, rules ↔ Nail rules where expressible, OOF ↔
vacation settings, To Do ↔ tasks, rooms in the free/busy picker), account
setup via Autodiscover with engine choice (Graph for Exchange Online, EWS
for on-premises), migration notes from the design (EWS retirement in
Exchange Online)." Scope decisions for M3: on-premises sign-in uses Windows
SSPI through the `windows` crate (no NTLM of our own); no non-loopback
sockets, DNS through the OS resolver (ADR 0057); no new GPL-family
dependency, no OpenSSL. The contract (ADR 0072) fixed the seams this
package fills: `NegotiateAuth` / `NegotiateSession`, the
`nail_api::exchange::pim` shapes, `ExchangeInboxRule`, automatic replies,
rooms, categories, `ExchangeMessageInfo`.

`crates/nail-exchange` is the only directory this package owns (plus its
texts in `locales/*/core-exchange.ftl`); everything the core, UI or
contract needs is requested from WP95.

## Decisions

### No new dependencies

Everything uses crates already in the workspace and the manifest WP90
wrote: `reqwest` + `nail-auth` (rustls / ring), `quick-xml` (POX),
`serde_json` (v2), `nail-discovery`'s `DnsLookup` (SRV through `SystemDns`),
`nail-ical` (Windows zones → IANA, `RRULE` parsing, SHA-256), `chrono` /
`chrono-tz`, `base64`, `windows` (SSPI). The SSPI identity structure
`SEC_WINNT_AUTH_IDENTITY_W` is declared in the crate (`#[repr(C)]`) rather
than enabling `Win32_System_Rpc`.

### HTTP seam (`http.rs`)

`ExchangeHttp` sends one request and never follows redirects; the callers
check every hop. `ReqwestHttp` wraps a `nail_auth::http_client()`;
`route_all_to(loopback)` sends every request to a mock server keeping the
real host in `Host` (as `nail-discovery`'s `ReqwestFetch` does), so tests
and demo runs use real host names. Bodies are capped at 1 MiB. Request
`Debug` output redacts `Authorization`.

### Windows sign-in (`negotiate.rs`, `sspi.rs`)

- `negotiate::send_signed_in` / `answer_challenge` run RFC 4559 over HTTP
  401: unauthenticated first; `Negotiate` (preferred) or `NTLM` (when the
  server offers only that) through any `NegotiateAuth`; tokens fed back
  until a non-401 answer; a final mutual-authentication token is verified;
  at most 6 rounds; a bare 401 after a token = `Rejected` (`auth_failed`).
  Basic only over `https` (or loopback `http`). `WWW-Authenticate` parsing
  handles several challenges per header, quoted parameters and token68.
- `sspi::SspiNegotiate` implements `NegotiateAuth`:
  `AcquireCredentialsHandleW` (signed-in user = no identity; explicit =
  `DOMAIN\user` split, UPNs whole, the password buffer zeroed after the
  call), one `InitializeSecurityContextW` per `step` (`ISC_REQ_CONNECTION |
  ISC_REQ_MUTUAL_AUTH`, output buffer sized from `cbMaxToken`,
  `CompleteAuthToken` where asked), handles released on drop. `login()`
  pairs a Negotiate context factory with an NTLM one for NTLM-only servers
  (Kerberos never falls back). `from_auth` reads `auth "windows"`.
  Off Windows every call answers `unsupported`.
- Tests: scripted fakes for the loop (all branches), the NTLM first token
  (`NTLMSSP\0`, type 1) offline, and the **real** client against a real SSPI
  server in-process (`AcceptSecurityContext`, test-only) for NTLM and
  Negotiate with the signed-in user, plus a rejected explicit login — no
  network. One ignored live test (`tests/sspi_live.rs`) signs in to a real
  Exchange Server from `NAIL_EWS_LIVE_URL` (owner-run).

### Autodiscover with engine choice (`autodiscover/`)

Order: v2 JSON (no login) at `autodiscover.<domain>`, `<domain>`, then
Microsoft's global `autodiscover-s.outlook.com` (switchable; it reveals the
address to Microsoft, as Outlook does); POX (with the login) at
`https://<domain>/…` and `https://autodiscover.<domain>/…`, following
`redirectAddr` (a v2 lookup for the new address first — hybrid users land
in Exchange Online without sending credentials), `redirectUrl` and HTTP
redirects; then the HTTP redirect method (`http://autodiscover.<domain>`)
and SRV `_autodiscover._tcp.<domain>`. SCP lookup in Active Directory is
out of scope.

**Credential rule.** Credentials go only to hosts equal to or below the
address's domain, or to hosts the user confirmed. Any other host that asks
to sign in ends the run with `ConfirmRedirect {url, host, domain, reason}`
(`http-redirect`, `redirect-url`, `redirect-addr`, `http-method`, `srv`)
before anything is sent; the UI asks (`exchange-autodiscover-confirm-redirect`)
and runs again with the host in `trusted_hosts`. Targets of the
unauthenticated pointers (HTTP method, SRV) always need confirmation, even
inside the domain. Only `https` hops are followed; at most 10 redirects
(`TooManyRedirects`). A server of the domain that asks to sign in when no
login was given answers `NeedsLogin {host, schemes}`; a rejected login is
an error (`auth_failed`), not "not found".

**Engine choice.** An EWS URL on Exchange Online (`*.office365.com`,
`outlook.office.com`, `*.outlook.com`) → Graph (OAuth2, provider
`microsoft`, host `graph.microsoft.com`); anything else → EWS with the
found URL (written only when it differs from `https://<host>/EWS/Exchange.asmx`),
auth `windows` (signed-in user, or the given user name) or `password` for a
Basic login. The external (`EXPR`) URL wins over the internal (`EXCH`).
IMAP + SMTP listed in POX are added as fallback proposals. Proposals get
`insecure-source` when a hop was plain HTTP and `foreign-hosts` for
on-premises hosts outside the domain. Automapped `AlternativeMailbox`es
(not archives, not the own address) become `ExchangeMailboxSettings`
(kind `shared`: Autodiscover cannot tell shared from delegated).

`MockAutodiscoverServer` (feature `testing`) plays several hosts behind one
loopback port: v2 answers / redirects / errors, POX settings / redirectAddr
/ redirectUrl / HTTP redirects, Basic or a scripted Negotiate / NTLM login
(`NEGOTIATE_TOKEN` → `CHALLENGE_TOKEN` → `AUTHENTICATE_TOKEN`, usable by
WP92's tests too), the HTTP redirect method, and a request log with
`Authorization` headers so tests prove where credentials went.

### Mapping (`mapping/`)

- **Categories ↔ tags**: case-insensitive names (Unicode lowercase), the
  master list's spelling wins; tags that are not valid category names
  (commas, semicolons, control characters, IMAP system keywords `$…` /
  `\…`) are not written as categories; `retag` for renames (merging onto
  an existing tag); duplicate-name check; `nearest_color` picks Outlook's
  swatch for a coloured Nail tag.
- **Focused Inbox**: only Inbox messages keep the classification;
  `focus:` matches nothing else; `needs_move` skips unchanged messages.
- **Rules**: a Nail rule is placed when its tree is a leaf, an `all` of
  leaves and `not`s, an `any` of same-kind leaves, or a `not` of those;
  leaves: sender / recipients address equals (an address) or contains,
  subject / body contains, header exists / list mail, has attachments,
  size (whole KB), inbox / own account; case-sensitive tests, regexes,
  starts / ends with, To-only or Cc-only, Reply-To / Sender, header values,
  tags, flags, dates, ages, auth results and queries are not expressible.
  Actions: move / copy (own account, resolved folder; unknown folder =
  `client` with `folder-unknown`), archive, delete, tag → category, mark
  read, forward (default identity), redirect, stop (later actions never
  run). Disabled, non-arrival / extra triggers, foreign scopes and
  `server-rules=#false` give `client`. Reasons carry the condition position
  / action index like `nail-sieve`. `digest` fingerprints a placed rule;
  `nail_rule_of` shows a server rule as a Nail rule where possible
  (round-trip tested).
- **Automatic replies ↔ vacation**: dates are the mailbox zone's days (end
  inclusive ↔ exclusive UTC instants, DST-safe); body → both replies
  (external audience kept); subject, days, addresses, `from` and differing
  external replies are reported as notes (`exchange-oof-note-*`); an open
  end becomes 365 days.
- **Events**: Windows / IANA zones → IANA (`nail_ical::tz`; unknown → UTC
  with a note), all-day dates, patterns ↔ `RRULE` both ways (daily,
  weekly incl. "every weekday", absolute / relative monthly and yearly,
  `BYSETPOS` for multi-day relative patterns; end date ↔ `UNTIL` at the
  end of the last day in the series zone), cancelled occurrences ↔
  `EXDATE`, exceptions ↔ overrides (original start converted into the
  series zone), attendees (rooms from room locations as `CUTYPE=ROOM`),
  show-as / sensitivity / importance / reminder / online meeting / categories.
  Series Exchange cannot store (several rules, `RDATE`, `BYWEEKNO`, …,
  `THISANDFUTURE`) answer `NotExpressible(exchange-pim-reason-*)`.
- **Tasks / To Do**: checklist items ↔ one level of subtasks (stable
  derived `UID`s), midnight dates ↔ all-day dates, reminders ↔ absolute
  alarms, importance normal ↔ no priority, linked resources kept.
- **Contacts**: Exchange slots ↔ vCard-like lists (three emails, typed
  phones and addresses, company / department, birthday with Outlook's 1604
  "no year"), distribution lists ↔ groups by address, JPEG / PNG photos.
- **Rooms**: filter by room list and capacity, availability from the
  account's free/busy (clipped, merged; free rooms first; unknown when
  unanswered), `room_attendee` / `book_room`.

### EWS retirement in Exchange Online (migration notes)

Microsoft announced (2023) that EWS requests to **Exchange Online** stop
working from **2026-10-01**, with Microsoft Graph as the replacement;
Exchange Server on-premises (2016 / 2019 / Subscription Edition) keeps EWS.
This is from memory and not re-checked against Microsoft's current
announcements (later updates may stage the shutdown or allow temporary
exceptions per tenant) — **maintainer task: confirm the date and any
exceptions.** Consequences in Nail:

- Autodiscover never proposes EWS for Exchange Online; it proposes Graph.
- An existing `engine="ews"` account whose host is Exchange Online keeps
  working as long as Microsoft allows it but shows
  `exchange-warning-ews-retired-online` (contract); switching it means
  `engine="graph"` and an OAuth sign-in with the Graph scopes (tokens are
  per resource, so the EWS refresh token cannot be reused).
- Hybrid users whose on-premises Autodiscover redirects to Exchange Online
  (`redirectAddr`) end up on Graph without credentials being sent to
  Microsoft's Autodiscover.
- Graph has no streaming push for desktop clients (webhooks need a public
  endpoint): Exchange Online accounts poll deltas (contract).
- Graph has no Autodiscover equivalent of `AlternativeMailbox`: automapped
  shared mailboxes are found only through on-premises POX; Exchange
  Online users add shared mailboxes by address (contract requests).

## Consequences

- WP95 wires: `Autodiscoverer` into account setup (the `DiscoverAccount`
  flow needs the confirm / login steps on the wire — requested), the EWS
  engine's `NegotiateAuth` from `SspiNegotiate::from_auth(...).login()`,
  the mapping functions between the engines and the store, rule placement
  into `GetExchangeRules`, the vacation UI for Exchange accounts through
  `mapping::oof`, `focus:` with `mapping::focus`, rooms in `FindRooms`.
- `cargo test -p nail-exchange` runs real SSPI handshakes in-process on
  Windows (NTLM, Negotiate); machines whose policy forbids NTLM would fail
  those tests (none known in CI).
- Not done here: SCP lookup; Autodiscover for ActiveSync; per-address POX
  answers in the mock; Kerberos-only end-to-end tests (need a domain);
  delegated vs shared detection for automapped mailboxes.
