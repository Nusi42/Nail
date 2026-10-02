# Nail user documentation (English)

Guides and references for people using Nail.

| Page | What it covers |
| --- | --- |
| [Release notes](release-notes.md) | What is new in Nail 1.0, what changed for existing data, known limits |
| [Getting started](getting-started.md) | Starting Nail (desktop or browser), the demo mailbox, finding your way around, privacy when reading mail, where settings and mail are kept |
| [Accounts](accounts.md) | Adding an account, signing in, account and identity settings, the unified inbox, virtual accounts, connection states |
| [Reading and organizing mail](mail.md) | Conversations, flags, archive, move, delete, keyboard shortcuts, snooze, junk mail and the junk filter, messages downloaded on demand, the offline queue |
| [Calendar, contacts and tasks](calendar-contacts-tasks.md) | Calendar views and the event editor (repeating events: this / this and following / all), reminders, contacts, groups and merging duplicates, tasks with subtasks, subscriptions, CalDAV / CardDAV servers and conflicts, importing `.ics` / `.vcf`, events and tasks from email, what Nail keeps of calendar files |
| [Invitations, scheduling and Gmail labels](invitations-and-gmail-labels.md) | The invitation card (answering, proposing a new time, the spoofing warning, the mail Nail sends, calendar servers that schedule themselves), organizing meetings, finding a time with free/busy, Gmail accounts, labels and categories |
| [Microsoft Exchange and Microsoft 365](exchange.md) | Adding an Exchange Online or Exchange Server account (Autodiscover, signing in), shared mailboxes and delegate access, categories, Focused Inbox, automatic replies, rules on the server, booking a room, Microsoft To Do |
| [Searching](search.md) | The search bar and the search language: fields, AND / OR / NOT, quotes, error messages |
| [Writing mail](composer.md) | The composer: plain text, HTML and Markdown, attachments and the reminder, replies and signatures, drafts saved to the server, undo send, send later, the outbox |
| [Markdown mail](markdown-mail.md) | What Markdown can do, what recipients get, the mail stylesheet and code colours per identity |
| [Accessibility](accessibility.md) | Keyboard use, context menus (Shift+F10), shortcut / toolbar / column editors, screen readers, the high-contrast theme, reduced motion |
| [Filter rules](rules.md) | Rules that sort, tag and answer mail: when they run, conditions, actions, testing, undoing a run, `rules.kdl` |
| [Server-side rules and vacation replies](server-side-rules.md) | Rules that run on your mail server (Sieve / ManageSieve), which rules can run there and why others stay in Nail, the vacation reply, what Nail changes on the server |
| [Junk mail, snooze and notifications](junk-snooze-notifications.md) | The server's Junk flags and Nail's own junk filter, snoozing and waking messages, new-mail notifications, quiet hours, per-account / folder / rule settings |
| [Signing, encryption and data protection](security.md) | Keys and certificates, trust, defaults per address, composer toggles, reader badges and warnings, OpenPGP and Autocrypt, S/MIME, encryption at rest, Autocrypt setup |
| [Recovery key](recovery-key.md) | Creating, keeping and checking the recovery key of encrypted data, opening your data with it after the key in Windows was lost, replacing or removing it |
| [Smartcards](smartcards.md) | Signing and decrypting with keys on a smartcard or token (PKCS#11): modules, the PIN prompt and PIN cache, using card keys for S/MIME and OpenPGP, what is not supported |
| [Data protection in detail](data-protection.md) | What encryption at rest covers, the key, switching it on and off, searching encrypted mail, the sender check, link and attachment warnings |
| [Remote access and remote content](remote-access.md) | Turning remote access on, internet mode (locked), users, passwords, two-step verification and passkeys, pairing a device, signing in, devices and sessions, the security log, loading remote content per message / sender / domain, the privacy list |
| [Remote sign-in security](remote-sign-in-security.md) | How passwords, verification codes, passkeys and pairing codes are protected, sessions and their limits, re-authentication, rate limits and lockouts, what the security log records |
| [Secure connections and remote content](remote-connections.md) | HTTPS and the certificate fingerprint, allowed networks, what the browser is told, mail shown from a separate address, how remote images are fetched through Nail |
| [Connecting AI agents (MCP)](mcp.md) | MCP clients, tokens, permissions, configuring agents (HTTP and `nail mcp`), resources and tools, write tools and whether they ask, confirmations, prompts, limits, the activity log, safety |
| [Confirmations](confirmations.md) | Approving or denying changes AI agents and plug-ins want to make, "always allow", what may run without asking, the history |
| [Importing and exporting](import-export.md) | Bringing in mail from mbox, EML folders, Maildir and Thunderbird (accounts, folders, address books, filters), calendars and contacts; exporting folders, calendars and address books; reports and what was skipped |
| [Backups and restoring](backup-restore.md) | What a backup contains, the passphrase and encryption, checking a backup, the dry run before restoring, what a restore replaces or merges |
| [Diagnostics bundle](diagnostics.md) | Creating a bundle for support: choosing what goes in, the exact preview, what is always removed or hidden, opting in to messages |
| [Command palette](command-palette.md) | Ctrl+K: every action in one searchable list, fuzzy search, shortcuts, recent actions, actions that cannot run right now |
| [Printing](printing.md) | Printing messages, conversations, events and agendas, the print options, saving as PDF on the desktop and in a browser |
| [Customizing](customization.md) | The inspector, dividers written to `layout.kdl`, editing toolbars, shortcuts and list columns, right-click menus, applying a shared theme or layout (merge dialog, backups), every config error at once, smart folders |
| [Plug-ins](plugins/README.md) | Installing and managing plug-ins, permissions, security, placing and styling them; writing plug-ins: getting started, the sandbox, manifest, writing data, mail hooks, composer, attachments and links, background jobs, tools for AI agents, translations, API and component references |
| [Installing Nail on Windows](installing.md) | The downloads (x64 / ARM64, offline, MSI, portable), installer options, Default apps, silent install, updates, uninstall and your data, the portable version |
| [The desktop app on Windows](desktop-app.md) | The welcome screen, updates and About Nail, tray and starting with Windows, default apps, mail from other programs ("Send to → Mail recipient"; never sent without you) |
| [Deploying Nail](deploying-nail.md) | For IT: which package, silent per-user and per-machine (MSI) installs and their options, update channels and policies, "Send to > Mail recipient", winget |
| [Theme reference](theme-reference.md) | `theme.kdl`: colours, fonts, spacing, parts and states, icons, message display |
| [Layout reference](layout-reference.md) | `layout.kdl`: windows, panes, widgets, actions, keyboard shortcuts, toolbars, menus |
| [Row templates, parts and keyboard](templates-and-parts.md) | Message list row templates and pipes, every styleable part and state, keyboard navigation |
| [Accounts reference](accounts-reference.md) | `accounts.kdl`: accounts, servers and sign-in, identities (formats, signatures, mail style), what to synchronize, virtual accounts, OAuth2 providers, undo send |

These pages describe Nail 1.0 (release candidate `1.0.0-rc.1`); what is new is in the [release notes](release-notes.md).

Internal material (design, plan, decisions) lives in [`docs/`](../../docs/)
instead. Translations go to `userDocs/<language>/`; a page without a
translation is read in English.
