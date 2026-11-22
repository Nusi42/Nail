# Changelog

All notable changes to Nail. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/). The user-facing release notes
are in [`userDocs/en/release-notes.md`](userDocs/en/release-notes.md);
decisions behind each change are in [`docs/adr/`](docs/adr/).

## [Unreleased]

### Fixed — bulk actions (ADR 0230)

- The status bar's "Saving N changes" counts the core's real queue of
  message writes (new request `GetOpsSummary`, event `OpsChanged` once per
  queued batch and per replayed group instead of one event per operation),
  so it no longer stays up after several hundred moves or deletes.
- A client that missed events (more than the event channel holds) gets the
  queue summary and a new `Resync` event and refetches its open message
  lists and selection, so moved messages no longer linger in other lists.
- Failed writes can be retried or dismissed all at once
  (`RetryFailedOps`, `DismissFailedOps`). The status bar names the newest
  failure's real reason (its error class and text are stored with the
  write, schema v22, so they survive a restart) and how many failed.
- Message writes no longer send one `OpStatus` event per operation;
  `OpsChanged` replaces them (folder changes keep `OpStatus`). Waiting
  folder changes, Gmail label writes and Exchange writes are counted in the
  summary. A pass that has to wait logs one INFO line.
- The status bar asks the core again when changes wait and no summary
  arrived for a while, so it returns to idle even when events are lost.
- Moved messages no longer show again in their old folder when the server
  rebuilt it (UIDVALIDITY change) while the move was still waiting.
- The Move window moves only to the highlighted folder or an exact name
  match; better matches come first, the current folder is left out, and
  moving into Drafts, Trash or Junk asks first with the count.
- Only `\Draft` messages continue in the composer; other messages in a
  Drafts folder open in the reader, with the new action Move out of
  Drafts. `EditDraftMessage` on a message without `\Draft` makes a new
  draft that never replaces or deletes it.
- A draft's server copy is deleted only when it is Nail's own copy or
  flagged `\Draft`; schema v23 unlinks drafts that 0.8.8 linked to a
  received message, which their next autosave, send or discard deleted
  (ADR 0233). Move out of Drafts moves only the selected message.
- A server message held back while a move of its Message-ID out of the
  folder waits or failed is listed (schema v23, `sync_refetch`) and fetched
  once nothing holds it back; dismissing a failed move or delete puts the
  message back where the server still has it (ADR 0233).

## [0.8.8] — 2026-11-21

### Added — rules on demand (ADRs 0219, 0221)

- Run one rule, or all rules that are on, on the selected messages or a
  folder: from the Run rule submenu in the message and folder menus, from
  the rules window's Run on choice, and for agents through the MCP tool
  `run_rules` (forwards and replies only on named messages, excluded
  folders respected).

### Changed — rules window (ADRs 0222, 0223)

- Redesigned rules window: a footer that stays in view (Run on, Run, Test,
  Save, Delete with confirmation), a problem badge instead of red error
  blocks, Preview and Recent runs tabs (actions with their values, why a
  message matched, Apply to these, run time and target), a compact
  When / Where / If / Then form, rule search, drag and Alt+Up/Down
  reordering, starter rules, and the same look in Settings. Asks before
  closing with unsaved changes.
- New rules switch between Editor and Text without errors; unsaved rules
  no longer get an empty id.

### Added — pickers and values from the mail (ADRs 0224, 0225)

- Searchable pickers for folders (a tree), accounts, tags, addresses,
  mailing lists and header names, with free text still allowed, in rules,
  virtual folders, search and settings.
- Create rule from message and Add to rule in the mail menus; condition
  fields offer the mail's sender, domain, recipients, subject, list,
  folder and tags.

### Changed — search (ADRs 0220, 0226)

- Search runs while typing (Enter still searches at once). Query mistakes
  show as a small badge with hover text instead of moving the layout;
  search hints and values float above the list.

### Changed — rules window decisions (ADRs 0227, 0229)

- Select several rules in the rules window (Ctrl+click, Shift+click,
  Shift+arrows, Ctrl+arrows then Space, Ctrl+A); Run runs them all in list order with one Undo,
  Test previews each rule, grouped by rule.
- One Exchange badge per rule: the number sums the reasons over all
  Exchange accounts; hover, or the row with the keyboard focus, lists each
  account (Escape hides that list).
- High contrast: a selected rule row keeps its handle, triggers, badge and
  On mark readable, and the rule the editor shows is marked among several.
- "Values from this mail" sits right after a condition's value, before
  "Match upper and lower case".
- The Run on choice stays while you pick other rules.
- The rule list's drag handle and On mark are pointer-only; the keyboard
  reorders with Alt+Up/Down and switches a rule in the editor's On switch.

### Changed — pickers everywhere (ADRs 0227, 0228)

- Account, folder and address fields in Settings use the shared picker
  everywhere (junk, server rules and vacation account, Also my addresses,
  searchable decrypted mail, MCP accounts and hidden folders, import and
  export, key lookup, automatic replies). Multi-value fields show chips.
- An empty address field lists your most-used addresses; the vacation
  reply's "Reply from" and "Also my addresses" offer only your own.
- Multi-value fields tell screen readers what they hold and announce each
  change; chosen options show a check mark.

### Added — Paper theme

- New built-in light theme Paper: warm paper neutrals with a blue accent.

## [0.8.6] — 2026-11-19

### Fixed — agents (MCP) and drafts (ADR 0217)

- `update_draft` and `send_message` also find a draft by the message id
  agents see when they read it, so drafts written in Nail or in an earlier
  agent session can be changed and sent.
- An agent whose own prompt was declined without asking (some agents do
  that when they cannot show it) no longer denies the send or delete: the
  request waits for your answer in Nail. The agent is no longer told that
  you denied it in Nail when you did not.

## [0.8.5] — 2026-11-18

### Fixed — continuing drafts

- A saved draft can be continued from the Drafts folder: double-click or
  Enter opens it in the composer instead of the reader, and the reader and
  the message menus offer Continue writing. Drafts written in another mail
  program open the same way; Nail takes them over as its own draft and
  replaces the server copy on the next save. The Outbox favorite also shows
  while saved drafts exist.

### Security (ADR 0216)

- ammonia 4.2.5 (mutation XSS through SVG and MathML, RUSTSEC-2026-0533),
  lettre 0.11.24 (STARTTLS reply injection, SMTP command injection, quoted
  local parts and denial of service, RUSTSEC-2026-0560 to 0566) and rmcp
  3.11.0 (OAuth SSRF filter and unbounded buffering, RUSTSEC-2026-0457,
  0461, 0462). chrono and chrono-tz are accepted as unmaintained.

## [0.8.4] — 2026-11-04

### Fixed (ADRs 0211, 0212)

- Links in HTML mail work on macOS and Linux: hovering shows the target, a
  click opens it (or asks first for a warned link). The text menu over mail
  (Copy, Select all) opens there too.
- On macOS and Linux, Nail's labels, buttons and lists can no longer be
  selected like text (the WebKit engine needs its own name for the rule).
- Open and Save on an attachment in the remote UI download the file instead
  of failing; every desktop-only request answers a remote session with
  `unsupported`, so the browser's own way is taken.
- An opened attachment with a very long name keeps its file type: the name
  is shortened before the extension.
- "Save as PDF…" in the print dialog asks for no file on macOS and Linux,
  where Nail cannot write the PDF itself, and points to the print dialog's
  PDF printer at once.
- A web address with an apostrophe in plain-text mail (`/it's-here`) is one
  link; an apostrophe around it is still left out.
- Opening a link or an attachment no longer blocks one of the core's worker
  threads while macOS starts the program.

### Changed (ADR 0212)

- Every mail frame runs one fixed script, by its hash, in an isolated origin;
  the remote UI gets Nail's text menu over mail as well.

### Security (ADR 0211)

- An opened attachment is marked as downloaded from the internet (the
  `Zone.Identifier` stream on Windows, the `com.apple.quarantine` attribute
  on macOS), so SmartScreen, Office's Protected View and Gatekeeper check it
  like a downloaded file.

## [0.8.3] — 2026-11-02

### Added (ADR 0211)

- A click on an attachment's name (or its new Open button) opens the file
  with the system's program; a flagged attachment asks first. In a browser
  the file downloads.
- URLs in plain-text mail are links.

### Fixed (ADR 0211)

- "Save as" for attachments works on macOS and Linux (native file dialogs
  through `rfd`).
- A link or file the system could not open is reported on macOS instead of
  being dropped silently.

## [0.8.2] — 2026-11-01

### Security (ADR 0209)

- Plug-in packages and backups are read with `zip` 9.2 and refused when
  their local headers disagree with the central directory or a name occurs
  twice, so no other zip reader can see different files than the ones Nail
  checked. A restored plug-in needs a valid plug-in id.
- Updated `rustls` (0.23.46), the mail HTML sanitizer `ammonia` (4.2.4),
  `rustls-webpki`, `webpki-roots`, `tokio`, `zstd` and the search engine
  `tantivy` (0.27) for published advisories; development tools updated
  (`vite`, `vitest`, `undici`, `markdownlint-cli2` and others).
- A release now stops while GitHub reports open Dependabot alerts, and
  checks the RustSec and npm advisories of what it ships (development
  setup only).

### Changed (ADR 0210)

- Update channels are gone: Nail asks one update address, the latest
  release on GitHub (`releases/latest/download/latest.json`), and
  `release:publish` adds `latest.json` to the version's own release and
  marks it Latest. Pre-releases are never offered as updates. The channel
  choice in Settings → Updates and "About Nail", the `UpdateChannel` policy
  value (now ignored) and the rolling `stable` / `beta` releases are
  removed.

## [0.8.1] — 2026-10-31

### Fixed (ADRs 0206, 0207)

- Links in HTML mail open outside the reader (the system browser in the
  desktop app, a new tab in the browser) instead of loading inside the
  message frame; the reader shows a hovered or focused link's target and
  asks before opening a link Nail warns about (ADR 0206).
- The message warnings list shows at most three entries, groups identical
  link warnings and keeps long targets on one line (ADR 0206).
- The desktop app reloads a window whose page crashed or hung, logs the
  failure and the page's errors and memory use, and shows an error screen
  instead of a blank window (ADR 0207).
- The remote-content buttons fit a narrow reader (labels end in an
  ellipsis); calendar reminder notices are capped.

### Changed (ADRs 0205, 0208)

- Only the master checkout publishes `main`, and branches are rebased,
  never merged (ADRs 0205, 0208; development setup only).

## [0.8.0] — 2026-10-26

The version is 0.8.0: the internal candidate `1.0.0-rc.1` was
never published and is renumbered; 1.0 follows once the gates in
[`docs/release-checklist.md`](docs/release-checklist.md) are passed.

### Added (ADR 0203)

- **Releases on GitHub**: `npm run release:prepare -- --version X.Y.Z`
  builds a release (version, changelog, full verification, the x64 NSIS
  installer and portable zip with update signatures, update manifest,
  release commit); `npm run release:publish` tags it, pushes it and creates
  the GitHub release. Installed copies find updates in the rolling `stable`
  and `beta` releases of the repository.
- The version has one source (`Cargo.toml`), and the release notes too:
  the GitHub release and the update dialog take their text from
  `userDocs/en/release-notes.md`.

### Changed (M5b, ADRs 0109–0114)

- **Navigation rail and views**: Mail, Calendar, Contacts, Tasks and
  Settings are views behind a rail on the left (Ctrl+1 … Ctrl+4, `g m`),
  with the unread count and pending confirmations as badges; Nail reopens
  the last view.
- **Mail view**: a folder pane with New message, Favorites, one section per
  account (with its colour) and smart folders; a list header with search,
  counts and the filter chips All / Unread / Flagged / Attachments; one
  ranked action bar with "More" and matching right-click menus.
- **Settings** are one searchable page grouped into General, Look & feel,
  Security and Extensions (the old tabs open as links to their section).
- **Calendar**: a one-line toolbar with a segmented view switch, calendar
  menus instead of buttons, readable event cards.
- Layouts of your own keep working; Nail offers the new default once
  through the merge dialog.

### Changed (post-M4c, ADR 0101)

- **Faster mail sync**: fetching, storing raw mail and indexing now overlap;
  a first sync of a 100,000-message mailbox takes about a third of the time,
  checking for new mail in a large mailbox about a quarter.
- **Replacing files from the Save dialog**: when Windows asks "Replace?"
  and you confirm, backups, recovery kits, diagnostics bundles and PDF
  files replace the existing file (atomically); a typed path to an existing
  file is still refused.

### Fixed

- A plug-in's CPU budget and hourly quota counted elapsed time, so on a busy
  PC a well-behaved plug-in could be stopped for "using too much CPU" while
  it was only waiting for a processor. They now count the CPU time of the
  plug-in's own thread (ADR 0204).
- A message you marked as junk could be reported as "marked by the server"
  when its arrival was classified after your mark (its `$Junk` keyword then
  looked like the server's verdict); your own verdict now always stands.
- The look-alike sender warning ran its two sentences together and showed
  its advice in the monospace style of link addresses.
- `nail serve --seed demo-multi` keeps its secrets in the demo keychain
  file, so encryption at rest survives a restart there too.
- Plug-in hot reload in `nail plugin dev` could stop watching a folder, or
  restart a plug-in that was just disabled, when changes arrived quickly.

## [1.0.0-rc.1] — 2026-10-02 (internal, renumbered 0.8.0)

The first release candidate for Windows: everything planned for version 1.0
(plan milestones M0–M4) is in. What still stands between this candidate and
1.0 are the human gates in [`docs/release-checklist.md`](docs/release-checklist.md)
(security review, a signed build, real-account tests, the first green CI
run, the release update key).

### Added — M4c: data portability, release hardening, leftovers (ADRs 0094–0099)

- **Import** from mbox (mboxrd / mboxo / mboxcl, streamed, any size), folders
  of `.eml` files, Maildir (Dovecot / Courier keywords, flags), Thunderbird
  profiles (local folders, POP3 mail, address books from `abook.sqlite` /
  `history.sqlite` / Mork `.mab`, accounts and filters offered as proposals,
  never passwords), `.ics` and `.vcf`; mail goes into **Local Folders** or an
  existing folder of an account (through its operation queue); repeated
  imports skip what is already there; per-item reports.
- **Export** of folders and messages to mbox, `.eml` or Maildir, calendars to
  `.ics`, address books to `.vcf`; an exported folder imports again
  byte-identically.
- **Backups**: one encrypted `.nailbackup` file (Argon2id + XChaCha20-Poly1305)
  with settings, theme, layout, rules, accounts (without secrets), local mail,
  local calendars and contacts, plug-ins and — only when chosen — keys;
  checking a backup, a dry run before restoring, restores that replace
  configuration files (kept as configuration backups) and merge the rest.
  Restored settings are checked like any change; smartcard, remote access
  and AI agent (MCP) settings stay on their computer.
- **Encryption at rest of the whole database file** (SQLite3 Multiple Ciphers)
  and the **vault recovery key**: shown once, checked, and the way back to the
  data after the key in the Windows Credential Manager was lost.
- **Smartcards and tokens (PKCS#11)** for S/MIME and OpenPGP signing and
  decryption, with PIN prompts in the UI; PINs are never stored.
- **Command palette** (Ctrl+K): every action, fuzzy search, shortcuts,
  recently used actions first.
- **Printing** of messages, conversations, events and agendas with a print
  look of the theme (`print` mode, `print.*` parts); **Save as PDF** in the
  desktop app (WebView2), the print dialog's PDF printer in a browser.
- **Diagnostics bundle** for support: exactly what goes in is shown first;
  passwords, tokens and keys are always removed, addresses, subjects and the
  user name in paths by default; mail only when picked.
- File dialogs of Windows for every path in the desktop app.
- Exchange and Gmail leftovers from M3: large messages (over 4 MB) go up to
  Microsoft Graph in pieces with send progress; send as / on behalf of shared
  mailboxes; server conversations as Nail threads; folder sync policies
  (headers only, recent days) for Exchange folders with bodies on demand;
  moving messages between Exchange mailboxes and between Exchange / Gmail
  accounts.
- A 100k-message performance benchmark with budgets (initial and incremental
  sync, search, list paging, cold start, memory, first render, scrolling).

### Fixed — M4c

- Initial synchronization was quadratic: the subject-fallback threading query
  scanned every message instead of its index.
- Moving a message from an IMAP or POP3 account into an Exchange or Gmail
  folder now files it on the server (it was only kept locally).

### Added — M4b: Windows integration and distribution (ADRs 0087–0093)

- Installers: per-user NSIS (x64 / ARM64, offline WebView2 variant), per-machine
  MSI with policies (`DISABLEUPDATES`, `AUTOSTART`, `ADDTOPATH`, `MAPI`), a
  portable zip, SBOMs, winget manifests.
- Updates with signed manifests (stable / beta channels, disabled by policy for
  MSI installs), tray icon, close to tray, start with Windows, default apps.
- Simple MAPI ("Send to → Mail recipient" from other programs): the composer
  opens with the files, nothing is ever sent without the user.
- The welcome screen and "About Nail".

### Added — M4a: agents and full extensibility (ADRs 0079–0086)

- AI agents (MCP) may write — drafts, flags, tags, moves, rules, calendar —
  through the operation queue; destructive and outgoing actions need a
  confirmation in Nail; policies per client; an activity log.
- Plug-ins: write permissions, mail hooks (arrival, before send, open),
  composer extensions, attachment previews, link actions, background jobs,
  MCP tools; the SDK and three example plug-ins.

### Added — M3: calendar, contacts, tasks, Gmail API, Exchange (ADRs 0058–0078)

- Calendars, contacts and tasks: local, ICS subscriptions, CalDAV / CardDAV;
  repeating events with this / this-and-following / all, reminders, contact
  merging, tasks with subtasks.
- Invitations and scheduling (iTIP / iMIP, free/busy, server-side scheduling).
- Gmail through the Gmail API (labels as tags, categories).
- Microsoft Exchange: Exchange Online (Graph) and Exchange Server (EWS, Windows
  sign-in), Autodiscover, shared mailboxes, categories, Focused Inbox,
  automatic replies, server rules, rooms, Microsoft To Do.

### Added — M2: security, remote UI, server-side rules, plug-in preview (ADRs 0029–0057)

- OpenPGP (with Autocrypt) and S/MIME: signing, encryption, key management,
  trust, warnings; encryption at rest of the data directory.
- The remote UI: HTTPS, users with passwords, two-step verification and
  passkeys, paired devices, a security log; remote content through a privacy
  proxy.
- Server-side rules (Sieve / ManageSieve) and vacation replies, junk filtering,
  snooze, notifications, folder sync policies and bodies on demand.
- Plug-ins in a sandbox (preview), the declarative plug-in UI, the plug-in
  manager.

### Added — M1: daily-driver mail (ADRs 0012–0028)

- IMAP (IDLE, CONDSTORE / QRESYNC), POP3, SMTP, OAuth2 sign-in, account
  discovery, the offline operation queue, the outbox with undo send, virtual
  (combined) accounts.
- The composer (plain text, HTML, Markdown), filter rules with undo, the MCP
  server for AI agents (read access), customization (inspector, toolbars,
  shortcuts, columns, shared themes and layouts).

### Added — M0: foundations (ADRs 0001–0011)

- The Rust core, the store, search, the React UI rendered from `layout.kdl` and
  styled by `theme.kdl`, the web mode (`nail serve`) and the Tauri desktop
  shell.

[0.8.8]: https://github.com/Nusi42/Nail/releases/tag/v0.8.8
[0.8.6]: https://github.com/Nusi42/Nail/releases/tag/v0.8.6
[0.8.5]: https://github.com/Nusi42/Nail/releases/tag/v0.8.5
[0.8.4]: https://github.com/Nusi42/Nail/releases/tag/v0.8.4
[0.8.3]: https://github.com/Nusi42/Nail/releases/tag/v0.8.3
[0.8.2]: https://github.com/Nusi42/Nail/releases/tag/v0.8.2
[0.8.1]: https://github.com/Nusi42/Nail/releases/tag/v0.8.1
[0.8.0]: https://github.com/Nusi42/Nail/releases/tag/v0.8.0
[1.0.0-rc.1]: docs/release-checklist.md
