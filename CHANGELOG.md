# Changelog

All notable changes to Nail. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/). The user-facing release notes
are in [`userDocs/en/release-notes.md`](userDocs/en/release-notes.md);
decisions behind each change are in [`docs/adr/`](docs/adr/).

## [Unreleased]

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

[0.8.1]: https://github.com/Nusi42/Nail/releases/tag/v0.8.1
[0.8.0]: https://github.com/Nusi42/Nail/releases/tag/v0.8.0
[1.0.0-rc.1]: docs/release-checklist.md
