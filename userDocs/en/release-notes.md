# Release notes

What changed in each version of Nail, newest first. Earlier milestones (the
preview versions) are summarized in the project's `CHANGELOG.md`.

## Nail 0.8.4 (`0.8.4`)

Links in mail work on macOS and Linux, and opened attachments are safer.

### Corrected in 0.8.4

- **Links in HTML mail on macOS and Linux** — pointing at a link shows where
  it goes, a click opens it in your browser, and a link that looks
  suspicious asks first, as on Windows.
- **The right-click menu over a message** (Copy, Select all) appears on
  macOS and Linux, and in the browser version too.
- **Labels and buttons** on macOS and Linux can no longer be selected like
  text by accident.
- **Attachments in the browser version** — Open and Save download the file
  instead of showing an error.
- **Long attachment names** keep their file type when opened, so the right
  program starts.
- **Save as PDF on macOS and Linux** points straight to the print dialog's
  PDF printer instead of asking for a file it cannot write.
- **Web addresses with an apostrophe** in plain-text mail are one link.

### Security in 0.8.4

- **Opened attachments are marked as downloaded from the internet**, so
  Windows SmartScreen, Office's Protected View and macOS Gatekeeper check
  them like any downloaded file.

## Nail 0.8.3 (`0.8.3`)

Open attachments directly, and file dialogs on macOS and Linux.

### Added

- **Open an attachment** — click an attachment's name, or its new Open
  button, to open the file with your system's program. An attachment Nail
  flags as dangerous asks first. In a browser the file downloads.
- **Links in plain-text mail** — web addresses in plain-text messages are
  links now.

### Corrected

- **Save as on macOS and Linux** — saving an attachment shows the system's
  file dialog, like on Windows.
- **Links that could not be opened** on macOS are reported instead of being
  dropped silently.

## Nail 0.8.2 (`0.8.2`)

A security update of the preview.

### Security

- **Plug-in packages and backups** are checked more strictly: Nail refuses
  an archive whose entries disagree with each other or that contains a
  name twice, so a crafted file cannot hide content from Nail's checks.
- **Mail with HTML** is cleaned with an updated sanitizer that closes two
  ways to sneak scripts into a message.
- **Secure connections** and other components are updated for published
  security advisories.

### Changed

- **One update address** — Nail looks for updates in the latest release
  on GitHub; the choice of an update channel in Settings → Updates is gone.

## Nail 0.8.1 (`0.8.1`)

A bug-fix update of the preview: links in mail, a window that turned white,
and the warnings above a message.

### Fixed

- **Links in mail open in your browser** — clicking a link in an HTML
  message used to load it inside the message, where most sites show only
  "This content is blocked". Now a click opens your default browser
  (`mailto:` links open the composer).
- **See where a link goes** — while the pointer is over a link, or the
  keyboard is on it, a small bar at the bottom of the reader shows its
  target. When the link shows one address but leads to another, the bar
  says so, and Nail asks before opening it. See [Mail](mail.md).
- **Shorter warnings** — the list of things to be careful about shows at
  most three entries (with **Show all**), joins identical warnings into one
  with a count, and keeps long addresses on one line.
- **No more white window** — when the page inside Nail's window crashes or
  stops responding, Nail now reloads it by itself and writes what happened
  to its log; an error while drawing shows a screen with **Reload** instead
  of a white window. `Ctrl+Shift+R` reloads the window.
- **Narrow windows** — the buttons for pictures from the internet no
  longer run past the edge of a narrow reader; long labels end in "…".
- Calendar reminders can no longer pile up without end.

## Nail 0.8 (`0.8.0`)

This is a preview of Nail for Windows: mail,
calendar, contacts and tasks with IMAP, POP3, Gmail and Microsoft Exchange,
end-to-end encryption, a customizable look and layout, plug-ins, AI agents
through MCP, and the remote UI. A preview is meant for testing:
please keep a [backup](backup-restore.md) of your data.

### New in this version

- **Real windows** — every window over the main window can be moved,
  resized from any edge or corner, maximized (double-click the title bar),
  snapped to the left or right half and moved / resized from the keyboard
  (`Alt+F7` / `Alt+F8`, `Ctrl+F6` between windows). Nail remembers where
  you put each kind of window. Tool windows no longer block the main
  window; questions block only the window they came from. See
  [Windows](windows.md).
- **Several messages at once** — each message has its own composer window,
  titled by its subject; in the desktop app a window can pop out into its
  own window on your desktop.
- **Delete permanently?** now asks before deleting, with the number of
  messages.
- **Conversation actions like Gmail** — Archive, Delete, Move, Snooze,
  Junk, read / unread and flags on a conversation row act on the whole
  conversation (Delete takes your replies in Sent along); one **Undo** in
  the notice (or `Z`) puts every message back. Each message in the
  conversation has its own **More** menu for actions on that message only.
- **HTML mail at its own height** — in the conversation feed short HTML
  mail takes only the room it needs and long newsletters show in full.
- **HTML mail as designed** — newsletters and mail from Outlook or Gmail
  keep their style sheets, colours and pictures sent inside the message.
- **Every window size** — the views, windows, dialogs and Settings adapt
  when you make Nail's window smaller or larger: windows stay on screen,
  form fields line up, and each dialog keeps its buttons visible at the
  bottom.
- **Import from other programs** — Thunderbird profiles (local folders,
  address books, and your accounts and filters as suggestions; passwords
  are never imported), mbox and Maildir files, folders of `.eml` files,
  calendar (`.ics`) and contact (`.vcf`) files. Imported mail goes into
  **Local Folders** on this computer or into a folder of one of your
  accounts. See [Importing and exporting](import-export.md).
- **Export** folders to mbox, `.eml` or Maildir, calendars to `.ics`, address
  books to `.vcf`.
- **Backups** — one encrypted file with your settings, look, layout, rules,
  accounts (without passwords), Local Folders, local calendars and contacts,
  plug-ins and, if you choose, your keys. Check a backup at any time; see
  what a restore would change before it changes anything. See
  [Backups and restoring](backup-restore.md).
- **The whole database is encrypted** when encryption at rest is on, and a
  **recovery key** gets your data back if the key in the Windows Credential
  Manager is lost. See [Recovery key](recovery-key.md).
- **Smartcards and tokens** for signing and decrypting with S/MIME and
  OpenPGP; Nail asks for the PIN when it needs it and never stores it. See
  [Smartcards](smartcards.md).
- **Command palette** — press **Ctrl+K** and type: every action in one list,
  with its shortcut. See [Command palette](command-palette.md).
- **Printing** of messages, conversations, events and agendas, and **Save as
  PDF** in the desktop app. See [Printing](printing.md).
- **Diagnostics bundle** for support: you see exactly what goes in before it
  is saved, and nothing is sent anywhere. See [Diagnostics bundle](diagnostics.md).
- **Exchange**: large messages (over 4 MB) are uploaded in pieces with
  progress; sending as or on behalf of a shared mailbox; folder sync
  policies; moving messages between mailboxes and to Gmail accounts. See
  [Microsoft Exchange and Microsoft 365](exchange.md).
- **Gmail and Exchange conversations** are shown as the server groups them.
- **Faster first sync of large mailboxes**: tested with 100,000 messages.
- **Faster start of the app**: the code that only the built-in demo needs
  is no longer loaded when you use your own accounts, and the rest loads in
  parallel parts. In web mode without remote sign-in, the browser console no
  longer shows an error when Nail checks whether a sign-in is needed.

### Changes to existing data

- If you use encryption at rest, Nail encrypts the database file itself on
  the first start of this version (in the background). Create a recovery
  key afterwards (**Settings → Recovery key**).
- Imported mail creates a new local account, **Local Folders**, on first
  use.

### Known limits

- Smartcard keys that require the PIN for every single use, and EdDSA /
  Curve 25519 keys on OpenPGP cards, are not supported yet.
- Thunderbird calendars are not imported directly; export them from
  Thunderbird as `.ics` and import that file.
- HTML mail is printed as text.
- Importing into an account files mail into the account's existing folders
  only; new folders are created only in Local Folders.
- Signed or encrypted messages over 4 MB cannot be sent through Microsoft
  Graph.
- The installers of this preview are not code-signed yet:
  Windows SmartScreen may warn when you start them. Choose **More info**,
  then **Run anyway**.
