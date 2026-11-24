# Backlog

Items that are agreed but not scheduled yet: deferred work, follow-ups and known gaps, collected from the follow-up
and deferral notes in `docs/plan.md`, the ADRs and the project memory. Move an item into `docs/plan.md` when it gets
scheduled, and remove it here when it is done.

Columns: **Source** names the `docs/plan.md` section (follow-up notes of a milestone or wave) and the ADR that
recorded the item. **Size** is a rough estimate: S = hours to a day, M = a few days, L = a week or more. Items marked
"(check)" may have been done or changed since they were recorded; confirm before scheduling them.

The release gates for 1.0 (security review, code signing, smartcard hardware, the update key, the external pentest,
the encryption-at-rest default) live in `docs/release-checklist.md` "Human gates" and are not repeated here.

## Mail and conversations

| Item | What is missing | Source | Size |
| --- | --- | --- | --- |
| Tag line facts in the list | `MessageSummary` has no attachment count, invitation or warning fields for the row's tag line (needs a store migration). | plan.md M5a, M5b follow-ups | M |
| Printing HTML mail | Print to PDF handles plain text only; HTML mail needs a measurable same-origin render. | plan.md M4c follow-ups (ADR 0099) | M |
| Tag states for a whole list | The label window shows no tag states when the whole list is selected. | plan.md R5 follow-ups (ADR 0237) | M |
| Snooze across installations | Snoozes are per installation: two installations on one account whose hidden folders share a name return each other's snoozed mail to the Inbox. | plan.md M2c follow-ups | M |
| Link actions in the reader | No `LinkActionMenu` in the reader: links live in the sandboxed frame (documented). | plan.md M4a follow-ups (ADR 0084) | M |
| Plain text in High contrast | The plain-text reader double-wraps 72-character lines at 1280 px. (check) | plan.md M5b follow-ups | S |
| Forwarding HTML with inline images | Forwards of HTML mail with inline images do not load previews. (check) | plan.md M2c follow-ups | S |

## Composer and sending

| Item | What is missing | Source | Size |
| --- | --- | --- | --- |
| Partial sends | When a Bcc copy fails after the visible copy went out, a retry sends the visible copy again. | plan.md M2a follow-ups (ADR 0034) | M |

## Rules, search and junk

| Item | What is missing | Source | Size |
| --- | --- | --- | --- |
| Other headers as rule values | `MessageFacts` offers no values of other headers. | plan.md R2 deferrals (ADR 0225) | M |
| Sieve while uploads fail | A rule edited while uploads fail runs in its old form on the server. | plan.md M2c follow-ups | M |

## Calendar, contacts and tasks

| Item | What is missing | Source | Size |
| --- | --- | --- | --- |
| Snooze alarms on the server | RFC 9074 snooze alarms are not written to the server (snooze is local; dismiss writes `ACKNOWLEDGED`). | plan.md M3a follow-ups | M |
| Task invitations | Only `VEVENT` scheduling; `VTODO` invitations show `no-event`. | plan.md M3b follow-ups (ADR 0066) | L |
| iTIP methods | `ADD` is applied but not built; `COUNTER` / `REFRESH` / `DECLINECOUNTER` always go by iMIP. | plan.md M3b follow-ups (ADR 0066) | M |
| Delegation | Delegation basics only (one delegate); delegates sending with `SENT-BY` are warned about, not applied (no delegate list). | plan.md M3b follow-ups (ADR 0068) | M |
| Schedule inbox | Schedule-inbox items are acknowledged, not applied (servers that only fill the inbox would need it). | plan.md M3b follow-ups (ADR 0068) | M |
| Occurrence answers | Answering an occurrence from the invitation card needs the event in a calendar. | plan.md M3b follow-ups | S |
| Exchange calendars | Exchange calendars cannot be created or renamed from Nail. | plan.md M3c follow-ups (ADR 0077) | M |
| EWS series time zones | EWS series in zones without a Windows id. | plan.md M3c follow-ups (ADR 0074) | M |
| Series outside the window | No periodic re-read of series outside the calendar window. | plan.md M3c follow-ups | S |
| Graph meeting creates | `MeetingSend::None` on Graph creates. | plan.md M3c follow-ups (ADR 0073) | S |
| To Do and Exchange contacts | To Do linked resources are not written back, subtasks only one level deep, no nested contact folders. | plan.md M3c follow-ups (ADR 0073) | M |
| Sealed PIM index | A sealed on-disk PIM index, if the in-memory one becomes too slow to build. | plan.md M3a follow-ups | L |

## Accounts and engines (IMAP, Gmail, Graph, EWS)

| Item | What is missing | Source | Size |
| --- | --- | --- | --- |
| Gmail listing | Raw messages of a listing are fetched one by one (no headers-only policy for Gmail). (check) | plan.md M3b follow-ups (ADR 0067) | M |
| Gmail drafts | `drafts.update` / `drafts.send` are unused. | plan.md M3b follow-ups (ADR 0067) | M |
| Gmail batched writes | `threads.modify` and batched `messages.trash` are unused. | plan.md P3 deferrals | S |
| Request limits across engines | No per-user request limiting across engines. | plan.md M3b follow-ups | M |
| Gmail API in the server editor | The account setup's server editor has no "Gmail API" protocol choice (discovery proposes it). | plan.md M3b follow-ups | S |
| Exchange sign-in and discovery | Autodiscover with a password or another Windows account; SCP lookup. | plan.md M3c follow-ups (ADR 0075) | M |
| Exchange delegate rights | Delegate rights (`GetDelegate`) are not shown in the UI. | plan.md M3c follow-ups | M |
| Focused Inbox | The Focused Inbox switch is per session. | plan.md M3c follow-ups (ADR 0076) | S |
| Discovery hardening | DNSSEC-validated SRV, a public-suffix list, DNS TXT discovery paths (needs a TXT lookup in `nail-discovery`). | plan.md "Carried items not in M2", M3a follow-ups | M |
| Atomic IMAP moves between accounts | Moves between IMAP accounts can duplicate a message (never lose it). (check) | plan.md "Carried items not in M2" | M |

## Sync and operation queue

| Item | What is missing | Source | Size |
| --- | --- | --- | --- |
| Other write failures in the summary | Failures of folder, Gmail label and Exchange writes are not in `OpsSummary.failed`; bulk Retry / Dismiss act on message writes only. | plan.md R4 follow-ups (ADR 0231) | M |
| Replay and sync loop | Replay and sync stay sequenced in one loop (the replay connection only keeps the main connection's mailbox). | plan.md M2c follow-ups (ADR 0048) | M |

## Windows, dialogs, settings and layout

| Item | What is missing | Source | Size |
| --- | --- | --- | --- |
| Remaining fields on the shared picker | Plug-in folder-picker nodes, Gmail's parent label, Exchange shared-mailbox address and room lists, the composer's From and the scheduling attendee field. | plan.md R3 deferrals (ADR 0228) | M |
| Ungranted account in a picker | A hidden folder of an MCP account that is no longer granted shows its id as the chip. | plan.md R3 deferrals | S |
| Appearance preview | Appearance has no live preview before Save. | plan.md R6 follow-ups (ADR 0242) | M |
| Settings drafts | The "New group" name of the contact groups window is not guarded (the window registers one close guard, the group editor's). | plan.md R6 follow-ups (ADR 0244) | S |
| Window close hook | A `beforeClose` hook in the window layer instead of the composer's session-sync close guard; a time-out for a pop-out page that never answers the OS close request. (check) | plan.md M5c follow-ups (ADR 0122) | M |
| Pop-out preparers | Pop-out preparers for the remaining stateful editors so they can opt in (the rules window has one). | plan.md M5c follow-ups (ADR 0119) | M |
| Pop-out key bindings | No default key bindings for `popout.open` / `popout.return`. | plan.md M5c follow-ups | S |
| Window manager | Resize handles outside the frame (they sit over the body's scrollbar); snapping to the top edge / quarters; a backwards window cycle; per-session geometry for remote browsers. | plan.md M5c follow-ups (ADR 0117) | M |
| Shell and views | Restoring the remembered view needs a view host (the fallback opens nothing); hidden views stay mounted; the UI state (last view, settings section) is shared between desktop and remote sessions. | plan.md M5b follow-ups (ADR 0110) | M |
| Rail | The unread badge relies on the "All accounts" virtual account; no visible "View" affordance on the rail (a rail button changes every visual baseline). | plan.md M5b, M5d follow-ups | S |
| Application menu | The application menu (`menu "app"`) is unused in the web UI. (check) | plan.md M2c follow-ups (ADR 0092) | S |
| Customize mode | Remembered collapse of panes; focus on a widget just added; hidden nodes nested in a pane are not shown; the main window's rail / view host is not customizable; docking above / below inside a stack wraps it in a new split. | plan.md M5d follow-ups (ADR 0133) | M |
| Theme editor | User theme files are not backed up before edits; the preview shows tokens, not part rules; a user theme's preview in the list shows its base palette, not its overrides. | plan.md M5a, M5d follow-ups (ADR 0135) | M |
| Theme files | A `theme.kdl` that no longer resolves on a newly active theme only logs a warning (no `ConfigErrors`); old user files with `mode=` mix palettes (offer a migration). (check) | plan.md M5a follow-ups (ADR 0104) | S |

## Plug-ins

| Item | What is missing | Source | Size |
| --- | --- | --- | --- |
| Runtime limits | No cumulative quota and no wall-clock limit for promises left pending (only `activate` has 30 s). | plan.md M2d follow-ups (ADR 0056) | M |
| Plug-in search | With decrypted indexing on, plug-in searches leave out every encrypted message; `nail.mail.search` is local-index only. | plan.md M2d follow-ups (ADR 0056) | M |
| Cc in lists | `Cc` only in `nail.mail.get`; lists and searches carry none. | plan.md M2d follow-ups | S |
| Panel values | Values sent by panel buttons are not filtered against the last rendered panel tree (dialog values are). | plan.md M2d follow-ups | S |
| Event routing | Plug-in dialog / notification events are broadcast to every connected UI and filtered by `client_id` in the UI, not in the transport. | plan.md M2d follow-ups | M |
| SDK | `@nail/plugin-api` is not published; `src/api.ts` mirrors `prelude.js` by hand; `checkManifest` / `checkTree` cover the main rules only. | plan.md M2d follow-ups (ADR 0053) | M |
| Generated SDK types | Generated SDK types for `PluginCollection` / `PluginContact` / the write inputs. (check) | plan.md M4a follow-ups (ADR 0082) | M |
| Markdown in plug-in trees | A CommonMark subset (no tables or nested lists); how `window.open` of Markdown links behaves in the Tauri webview is unverified. | plan.md M2d follow-ups (ADR 0052) | S |
| Merge dialog | The merge dialog cannot start from a given text, so the placements step reuses its requests. | plan.md M2d follow-ups | S |
| Hook filters | A per-message matcher for hook filters (now a folder search). | plan.md M4a follow-ups (ADR 0081) | M |
| Hook state across restarts | Vetoes, before-send headers and MRTR request states are in memory (lost on restart). | plan.md M4a follow-ups (ADR 0081) | M |
| Tool argument validation | Full JSON Schema validation of plug-in MCP tool arguments. | plan.md M4a follow-ups | M |
| PDF preview example | A PDF attachment preview example plug-in. | plan.md M4a follow-ups | S |

## MCP and agents

| Item | What is missing | Source | Size |
| --- | --- | --- | --- |
| Encrypted attachments over MCP | Attachments of encrypted mail are served from the raw (encrypted) message even with the `encrypted` grant. (check) | plan.md M2a follow-ups (ADR 0034) | M |
| Activity view | An "all entries" MCP activity view for plug-in writes without a client. | plan.md M4a follow-ups (ADR 0083) | S |
| Audit details | A sealed audit detail, if the activity log should name subjects. | plan.md M4a follow-ups | M |

## Security, encryption and remote UI

| Item | What is missing | Source | Size |
| --- | --- | --- | --- |
| S/MIME structure | S/MIME inside `multipart/mixed` (a list footer after a signed entity) is not unwrapped; S/MIME header protection is neither produced nor read. | plan.md M2a follow-ups (ADR 0031) | M |
| Header-protected subjects | They stay `...` in the message list; showing them needs an opt-in cache like decrypted indexing. | plan.md M2a follow-ups | M |
| Autocrypt from old mail | Autocrypt learns only from mail that arrives after a folder's first sync; an import of old mail builds no peer state. | plan.md M2a follow-ups (ADR 0030) | M |
| `authserv-id` suggestions | Can name the receiving server's own domain when the provider adds no `Authentication-Results` and does not strip forged ones. | plan.md M2a follow-ups (ADR 0035) | S |
| S/MIME revocation | No revocation checking for S/MIME certificates. | plan.md "Carried items not in M2" | M |
| SSPI channel binding | TLS channel binding (Extended Protection) for the Windows integrated sign-in. | plan.md M3c follow-ups (ADR 0078) | M |
| Smartcard keys | Keys that need the PIN for every use (`CKA_ALWAYS_AUTHENTICATE`) and EdDSA card keys. | plan.md M4c follow-ups (ADR 0099) | M |
| Re-open locked data | Re-opening a locked data directory (and a hot re-open after a recovery-key restore) without a restart. | plan.md M4c follow-ups (ADR 0099) | M |
| Login throttle window | 7 days because the store counts from the first failure; counting from the last failure would allow hours. | plan.md M2b follow-ups (ADR 0041) | S |
| Passkeys | Discoverable-only (no `allowCredentials` for non-resident security keys); `credProtect` is requested, not enforced. | plan.md M2b follow-ups (ADR 0041) | M |
| CSRF key | Per process: browsers fetch a new token after a restart. | plan.md M2b follow-ups | S |
| Remote proxy limits | The proxy limit is per session on the app origin but per client address on the sandbox origin; "load once" lapses after 15 minutes. | plan.md M2c follow-ups (ADR 0048) | S |
| Narrowed remote grants | Sessions narrowed to some accounts see no address suggestions, no rules, and a search total that counts only the page's hits; PIM answers are not narrowed per source. | plan.md M2b, M3a follow-ups (ADR 0042) | M |
| ACME | Wired but untested end to end (needs the network and port 80; internet mode stays locked). | plan.md M2b follow-ups (ADR 0038) | M |

## Desktop integration and installers

| Item | What is missing | Source | Size |
| --- | --- | --- | --- |
| MAPI registry values | Confirm which values the Windows MAPI stub reads (`DLLPathEx` vs `DLLPath`, HKCU vs HKLM default client) with a real `mapi32.dll` call. | plan.md M4b follow-ups (ADR 0088) | S |
| MAPI on ARM64 | An ARM64X `NailMapi.dll`, so emulated x64 programs on ARM64 can load it. | plan.md M4b follow-ups | M |
| MAPI callers | A low-integrity pipe label if sandboxed (AppContainer) callers matter; elevated callers fail instead of asking the shell to start Nail de-elevated; a squatted pipe name is only logged. | plan.md M4b follow-ups (ADR 0088) | M |
| MAPI install option | The NSIS MAPI option's elevated step runs only with a UAC prompt; updates do not refresh that copy. | plan.md M4b follow-ups (ADR 0089) | M |
| Policies | Group policies are read once at start. | plan.md M4b follow-ups (ADR 0090) | S |

## Data portability

| Item | What is missing | Source | Size |
| --- | --- | --- | --- |
| Folders on servers during imports | Imports cannot create folders on servers (the folder-create operation of ADR 0196 is not used by imports). | plan.md M4c follow-ups (ADR 0099) | M |
| Thunderbird calendars | Thunderbird calendars (`local.sqlite`) are not imported. | plan.md M4c follow-ups (ADR 0096) | M |

## Localization

| Item | What is missing | Source | Size |
| --- | --- | --- | --- |
| Library texts | Texts from libraries (KDL parser, OS errors, regex errors, search parser) stay English `detail` arguments. | plan.md M2c follow-ups (ADR 0046) | M |
| MCP audit texts | MCP audit entries map English details back to texts; a store column would be cleaner. | plan.md M2c follow-ups (ADR 0049) | S |
| Headless `nail mcp` language | Headless `nail mcp` does not read the Windows UI language. | plan.md M2c follow-ups | S |
| Invitation dates and locale | iMIP texts show dates numerically (no ICU-style formatter); the iTIP locale is the core's, not per identity. | plan.md M3b follow-ups (ADR 0066) | M |
| Edit issue texts | `edit-issue-*` texts could show the extra Fluent arguments (`detail`, `window`, `toolbar`, `kind`). | plan.md M5d follow-ups (ADR 0136) | S |
| Date inputs | Native date inputs follow the browser / OS language. | plan.md M2d follow-ups | S |

## Code quality and docs

| Item | What is missing | Source | Size |
| --- | --- | --- | --- |
| Shared debounce | Move the remaining composer / merge-dialog debounces onto `state/debounce`. (check) | plan.md Q2 follow-ups (ADR 0177) | S |
| Content lines | One content-line layer for `nail-dav` and `nail-ical`. (check) | plan.md Q2 follow-ups | S |
| Shared test data and texts | Fixture instants out of `nail-util`; the iTIP / crypto texts into `common.ftl` once their test harnesses load it. | plan.md Q2 follow-ups | S |
| Composer instances | Decide whether `defaults/layout.kdl` alone gives the composer's `instances`. | plan.md Q2 follow-ups | S |
| PIM menu keyboard | `PimMenu` duplicates part of the context menu's keyboard model. | plan.md M5b follow-ups | S |
| ADR note | A note in ADR 0164 that ADRs 0235 / 0236 replace its limits. | plan.md R5 follow-ups | S |
| README media | The README media still show five themes and the old rule list (`npm run readme:media`, then review). | plan.md R3 deferrals (ADR 0197) | S |

## Tests and flaky tests

| Item | What is missing | Source | Size |
| --- | --- | --- | --- |
| Interop against real servers | The interop suite (Dovecot, Stalwart, Radicale) and the ManageSieve tests against Dovecot Pigeonhole and Stalwart have not run (they need Docker locally). | plan.md M2c follow-ups, M3 gate (ADR 0201) | M |
| Smartcard PIN dialog scan | A Playwright scan of the smartcard PIN dialog (needs a soft token in `nail serve`). | plan.md M4c follow-ups (ADR 0100) | S |
| Auto-schedule e2e | No e2e for the RFC 6638 auto-schedule path (core tests cover it). | plan.md M3b follow-ups | S |
| Mock coverage | The Gmail mock's search terms are limited; the EWS mock has no monthly / yearly expansion. | plan.md M3b, M3c follow-ups | S |
| Rule list drag | A Playwright drag scenario for the rule list. | plan.md R2 deferrals | S |
| Select-all gate | A 1 200-row e2e variant of the select-all gate. | plan.md R5 follow-ups | S |
| Dialogs without e2e | E2e for Automatic replies, the mailbox editor, the classify dialog, the Exchange category / mailbox deletes and the Outbox discard question (unit tests only). | plan.md R6 follow-ups | M |
| OS-level desktop checks | The OS notification look, the tray's OS menu and `contextmenu` after Shift+F10 in WebView2 are not checked automatically. | plan.md M2c, M4b follow-ups | M |
| Exchange move race | A deterministic mock hook for the Exchange move race. (check) | plan.md M5c follow-ups (ADR 0120) | S |
| Flaky: m5d customize layout | e2e `m5d` "customize layout … Undo and Redo" times out under heavy load (passes alone). | memory r4-status, r6-status | S |
| Flaky: connection-log step | e2e `a11y-windows` connection-log step times out under heavy load (passes alone). | memory r6-status | S |
| Flaky: rules window | e2e `r2-rules-window` times out under heavy load (passes alone). | memory r6-status | S |
| Flaky: card height on WebKit | e2e `p3-card-height` times out on WebKit under load (passes alone). | plan.md R4 status | S |
| Flaky: Exchange core tests | `nail-core` `tests/exchange.rs` (categories / automatic replies / rooms, shared mailboxes / sending / free-busy, the two interrupted-listing tests) failed once each on a machine about 5x slower. | memory r5-status | S |
| Listing throughput budgets | Throughput budgets of the 520-message listing tests at very high parallel load. (check) | plan.md M5c follow-ups | S |

## Performance

| Item | What is missing | Source | Size |
| --- | --- | --- | --- |
| Smaller parts of large actions | A smaller store transaction inside a part, if the UI must stay snappier (each 10 000-message part holds the store about 1.6 s). | plan.md R5 follow-ups (ADR 0236) | M |
| Replay of large IMAP moves | Profile the replay of 25 000 IMAP moves (about 1.2 ms per message). | plan.md R5 follow-ups | M |

## After v1.0

| Item | What is missing | Source | Size |
| --- | --- | --- | --- |
| Plug-in and theme gallery | A curated plug-in / theme gallery ("curated when we're done"). | plan.md section 9 scope decisions, section 11 | L |
| macOS and Linux releases | Builds and installers for macOS and Linux (the code already builds and tests there). | plan.md section 11 (ADR 0202) | L |
| Mobile apps | Mobile clients. | plan.md section 11 | L |
| German translations | The German catalogs in `locales/de/` (every `.ftl` file, including the PIM, desktop, system and newer catalogs) and the German installer strings. | plan.md section 11, M2b–M5d follow-ups | L |
| Push relay | A push relay for Graph and Gmail (desktop clients poll with delta queries / history). | plan.md M3 scope decisions | L |
| Internet mode | Unlocking internet mode for the remote UI (needs the external pentest, release checklist gate 7). | plan.md M2 / M4 human gates | L |

## Real-account and real-server testing

Decided: the tests against real Google and Microsoft services wait in the backlog; until then the automated suites use mock servers and recorded fixtures.

| Item | What it covers | Notes |
| --- | --- | --- |
| Google test account | Gmail API engine, Google CalDAV/CardDAV, OAuth sign-in | Whoever runs these tests creates the account (phone verification, Google's terms). Then: Google Cloud project, Gmail API enabled, OAuth consent screen in "Testing" mode with the account as test user (full Gmail scope without app review), desktop OAuth client id into `accounts.kdl` `oauth_providers`. In Testing mode refresh tokens expire after about 7 days (check), so a longer trial needs re-sign-in. Google CalDAV / CardDAV also need the calendar and contacts scopes on the client. |
| Outlook.com account (free) | Graph engine for personal accounts: mail, calendar, contacts, To Do | Free Entra app registration. Does not cover shared mailboxes, rooms, business inbox rules / out of office, or EWS. |
| Microsoft 365 Business tenant | All of Exchange Online: shared mailboxes, rooms, rules, out of office, invitations between users | Business Basic trial (about one month free, then a small per-user fee; check current prices) with the free `onmicrosoft.com` domain, 2–3 users. Alternative: Microsoft 365 Developer Program sandbox, if eligible (eligibility was restricted, e.g. to Visual Studio subscribers — check). |
| grommunio in the interop run | EWS engine against a real EWS implementation | Open-source groupware with EWS, runs as a container next to Dovecot, Stalwart and Radicale. Complements, does not replace, a Microsoft Exchange Server. |
| Exchange Server lab (optional) | EWS + Windows integrated sign-in (SSPI) against Microsoft's server | Hyper-V on Windows 11 Pro: Windows Server evaluation, Active Directory, Exchange Server Subscription Edition in trial mode; about 16 GB RAM for the VMs. Only if on-premises customers matter. |
