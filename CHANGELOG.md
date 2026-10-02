# Changelog

All notable changes to Nail. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/). The user-facing release notes
are in [`userDocs/en/release-notes.md`](userDocs/en/release-notes.md);
decisions behind each change are in [`docs/adr/`](docs/adr/).

## [1.0.0-rc.1] — 2026-10-02

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

[1.0.0-rc.1]: docs/release-checklist.md
