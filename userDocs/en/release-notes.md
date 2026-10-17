# Release notes

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
  Windows SmartScreen may warn when you start them.

Earlier milestones (the preview versions) are summarized in the project's
`CHANGELOG.md`.
