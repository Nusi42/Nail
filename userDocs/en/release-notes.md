# Release notes

What changed in each version of Nail, newest first. Earlier milestones (the
preview versions) are summarized in the project's `CHANGELOG.md`.

## Nail 0.8.10 (`0.8.10`)

Select a whole message list and act on it at once, however large it is.

### New in 0.8.10

- **Select all with `Ctrl+A`** — in the message list, `Ctrl+A` (`Cmd+A` on
  macOS) selects the whole list: every conversation or message of the
  folder, the unified inbox, a search or a filter, including rows that are
  not loaded yet. The list shows how many are selected. `Ctrl`+click or
  `Shift`+click leaves single rows out again, and `Esc` clears the
  selection.
- **One question before acting on the whole list** — every action on a
  whole list (archive, move, delete, read, flag, labels, snooze, junk) asks
  once how many conversations and messages it affects, and its button names
  the action ("Archive all"). It then runs as one change with one **Undo**;
  mail that arrives while the question is open is left alone. **Select
  all** is also in the command palette.

### Corrected in 0.8.10

- **No more "Too many conversations at once"** — acting on many selected
  conversations (for example archiving or deleting several hundred) could
  fail with "Too many conversations at once; select fewer", although they
  held far fewer messages than Nail can handle. There is no limit on the
  number of conversations any more: a very large action runs in parts in
  the background and is still undone as one, and its confirmation shows the
  true number of messages.

## Nail 0.8.9 (`0.8.9`)

Moving or deleting hundreds of messages at once is reliable again: the
status bar shows what is really still being saved, message lists stay
right, and the Move window no longer moves mail somewhere you did not
mean.

### Corrected in 0.8.9

- **"Saving changes" ends when the changes are saved** — after moving or
  deleting many messages in a few steps, the status bar could keep saying
  "Saving 14 changes" until Nail was restarted, although everything had
  reached the server. It now shows the real number of changes still
  waiting, for all accounts together, including waiting folder changes,
  Gmail labels and Exchange changes, and it returns to idle once they are
  saved, even after very large bulk actions.
- **Failed changes say why** — when changes could not be saved, the status
  bar names the real reason (for example no connection, or the server
  refused), also after a restart, and with several failures how many there
  are. **Retry** and **Dismiss** act on all of them at once; after
  **Dismiss**, a message you moved or deleted shows again where the server
  still has it.
- **Lists stay right after large moves** — messages you moved could still
  show in their old folder, or in another folder, until a restart. Nail now
  refreshes the open lists and the selection when it notices it fell
  behind, and moved messages no longer linger as selected rows.
- **No moved messages back in their old folder** — when the server rebuilt
  a folder (a UIDVALIDITY change) while moves out of it were still being
  saved, the moved messages could show again in the old folder. They no
  longer do.
- **The Move window moves only where you choose** — `Enter` used to move to
  the first folder whose name merely contained what you typed. Now the
  arrow keys highlight a folder and `Enter` takes the highlighted one (or a
  folder whose name you typed in full); better matches come first, and the
  folder the mail is in is no longer offered. Moving into Drafts, Trash or
  Junk asks first, with the number of messages.
- **Received mail in Drafts opens in the reader** — any message in a Drafts
  folder used to open in the composer as a draft. Now only real drafts do;
  other messages there open in the reader, and **Move out of Drafts** (in
  the reader's toolbar, the message menu and the command palette) takes
  the selected message back to the Inbox (real drafts stay). Continuing such
  a message from another client or an agent now makes a new draft and never
  replaces or deletes the original.
- **Received mail in Drafts is no longer deleted** — in 0.8.8, continuing a
  received message that sat in a Drafts folder could delete that message
  for good when the draft was saved, sent or discarded. Nail now never
  deletes a message that is not a draft this way, and drafts linked like
  that in 0.8.8 are unlinked when you update.
- **No message hidden after a waiting move** — a second copy of a message
  (for example from a mailing list) that arrived while moving the first
  copy was still being saved now shows up once the move is done.

## Nail 0.8.8 (`0.8.8`)

Run your rules on exactly the mail you choose, a redesigned rules window,
and pickers instead of typing folder or account names. Search now runs while
you type, several rules run at once, and there is a new theme, Paper.

### New in 0.8.8

- **Run rules where you want** — right-click messages or a folder and pick
  **Run rule**, or choose **Run on** in the rules window: the selected
  messages, the current folder, or where the rule applies. A new rule can
  be saved and run in one step.
- **A clearer rules window** — the buttons stay at the bottom, problems
  show as a small badge you can hover, and **Test rule** shows its results
  right under the rule, with **Apply to these**. **Recent runs** show when
  and where each run happened, and Delete and closing with unsaved changes
  ask first.
- **Pick instead of type** — folders, accounts, tags, addresses, mailing
  lists and header names have a searchable list (folders as a tree). You
  can still type a value yourself.
- **Rules from a message** — **Create rule from message** and **Add to
  rule…** in the mail menu. Next to each condition, a button offers the
  mail's sender, domain, recipients, subject, list and folder.
- **Search while you type** — no need to press Enter. A mistake in the
  query shows as a small badge with an explanation, and the list stays
  where it is.
- **Several rules at once** — select rules in the rules window with
  Ctrl+click, Shift+click, Shift+arrows, Ctrl+arrows and Space, or Ctrl+A. **Run** runs them all in
  list order, and one Undo takes the whole run back. **Test** previews each
  rule, grouped by rule.
- **One Exchange badge per rule** — the number adds up the reasons over all
  your Exchange accounts. Hover it, or move the keyboard focus to the rule,
  to see each account.
- **Values button in the right place** — "Values from this mail" now sits
  right after a condition's value, before "Match upper and lower case".
- **Run on stays** — the Run on choice stays while you pick other rules.
- **Pickers everywhere** — account, folder and address fields in Settings
  use the same searchable picker: junk, server rules and vacation reply,
  Also my addresses, searchable decrypted mail, agent (MCP) accounts and
  hidden folders, import and export, key lookup and automatic replies.
  Fields that take several values show them as chips.
- **Your most-used addresses first** — an empty address field lists the
  addresses you use most, before you type anything. The vacation reply's
  "Reply from" and "Also my addresses" offer only your own addresses.
- **New theme: Paper** — a warm light theme with paper tones and a blue
  accent.

## Nail 0.8.6 (`0.8.6`)

AI agents can work with all your drafts again, and they ask before sending
or deleting instead of failing.

### Corrected in 0.8.6

- **Agents send and delete again** — when an agent (for example Claude
  Code) asked to send a draft or delete messages, the request was refused
  at once and the agent was told you had denied it, without any question.
  Now Nail asks you in its own window, as it should.
- **Agents find your drafts** — agents can change and send drafts you wrote
  in Nail, or that they wrote in an earlier session, using the id they see
  when they read your Drafts folder.

## Nail 0.8.5 (`0.8.5`)

Continue writing your drafts straight from the Drafts folder, plus security
updates for HTML mail and sending.

### Corrected in 0.8.5

- **Drafts open in the editor** — double-click a draft in the Drafts folder,
  or select it and press Enter, to go on writing it. The reader and the
  right-click menu offer **Continue writing** too.
- **Drafts from other mail programs** — a draft you started in webmail or on
  another device opens for editing as well. When Nail saves it, it replaces
  the old copy on the server, so you do not end up with two.
- **The Outbox in the sidebar** also appears while you have saved drafts,
  so they are easy to find.

### Security in 0.8.5

- **HTML mail is cleaned more thoroughly** — the part of Nail that removes
  scripts and other active content from HTML mail now also handles crafted
  SVG and MathML correctly.
- **Sending mail is safer** — a server or attacker on the network can no
  longer slip false replies past Nail when the connection is secured, mail
  text cannot be turned into server commands, and a recipient address in
  quotes reaches exactly that address.
- **Connections from AI tools (MCP)** are better protected against
  oversized requests and answers.

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
