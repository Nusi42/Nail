# Getting started

Nail is an email client you can shape completely. Its look comes from one
file, `theme.kdl`, and its structure (which panes exist, where they sit,
which keys do what) comes from another, `layout.kdl`. You can change either
while Nail is running and see the result immediately.

This guide covers the first steps. See
[What Nail can do](#what-nail-can-do) for everything else.

## Start Nail

Nail runs in two ways, and both show the same interface:

- **As a desktop app.** Start *Nail* like any other Windows program.
- **In your browser.** Run `nail serve --open` in a terminal. Nail starts in
  the background and opens `http://127.0.0.1:4780/` in your browser (when
  another program already uses port 4780, Nail picks a free port and opens
  that address instead; the terminal shows it). Only this computer can
  connect. Press `Ctrl+C` in the terminal to stop it.

### Try it with demo mail

To look around without an account, start Nail with the demo mailbox:

```sh
nail serve --seed demo --open
```

The demo contains about twenty messages in *INBOX*, *Sent* and *Archive*:
newsletters, a conversation thread, attachments, and a few deliberately
broken or hostile messages. They show how Nail protects you.

## Find your way around

The **navigation rail** on the left switches between Nail's views: **Mail**,
**Calendar**, **Contacts**, **Tasks** and, at the bottom, **Settings**. The
number on Mail is your unread mail in all inboxes; the shield button shows
how many changes by AI agents or plug-ins wait for your approval. Your
account's avatar sits at the very bottom. Nail opens the view you used last.

The Mail view has three panes:

1. **Folders** (left): **New message** on top, then **Favorites** (All
   inboxes, Flagged, Snoozed, Drafts — the same folders of every account
   together), then one section per account with its colour and its folders,
   and your smart folders once you saved a search. Click a section's heading
   to fold it away; Nail remembers which ones you folded. The number next to
   a folder is its count of unread messages.
2. **Messages** (middle): the search field, the folder's name with its
   counts, the filter chips **All**, **Unread**, **Flagged** and
   **Attachments**, and the messages, newest first. Unread messages are
   bold.
3. **Message** (right): the action bar for the message (Reply, Reply all
   and Forward with labels, the common actions as icons, the rest under
   **More**), the other messages of its conversation, and the selected
   message with its headers, attachments and text.

The status bar at the bottom shows whether Nail is up to date, the progress
of a running check for new mail, and the number of messages in the folder.

You can drag the dividers between the panes to resize them. When the window
is narrow, the panes become tabs. Actions without a button of their own
(filter rules, the outbox, adding an account, …) are in the command palette
(**Ctrl+K**).

### Keyboard shortcuts

| Key | Action |
| --- | --- |
| `j` / `↓` | Next message |
| `k` / `↑` | Previous message |
| `c` | New message |
| `r` / `a` / `f` | Reply / reply all / forward |
| `e` | Archive |
| `v` | Move to folder |
| `Delete` / `Shift+Delete` | Move to Trash / delete for good |
| `u` / `s` | Mark read or unread / flag or unflag |
| `/` | Search (`Esc` ends the search) |
| `Ctrl+Enter` | Send (in the composer) |
| `Esc` | Close the composer, settings or another window |
| `g` then `o` | Show the outbox |
| `Ctrl+,` | Settings |
| `F1` | About Nail (version, installation, updates; desktop app) |
| `F5` | Check for new mail |
| `Ctrl+B` | Show or hide the folder pane |
| `g` then `r` | Show or hide the message pane |
| `Ctrl+K` | The command palette: find and run any action by typing |
| `Ctrl+1` … `Ctrl+4` | Mail, Calendar, Contacts, Tasks |
| `g` then `m` | Back to Mail |
| `↑` / `↓`, `Home` / `End` | On the navigation rail: the previous / next view button, the first / last |
| `Ctrl+P` | Print the selected message |

You can change every shortcut in `layout.kdl`; see the
[layout reference](layout-reference.md#keys).

## Your privacy when reading mail

- Nail shows HTML mail in a locked-down area. Scripts, forms and embedded
  programs are removed and never run.
- **Remote images are blocked.** Many newsletters load images from the
  sender's server to find out when and where you opened the mail
  ("tracking pixels"). Nail does not load them. When it removes remote
  content, a note above the message says so.

## Make it yours

Nail reads two optional files from its configuration folder:

| File | What it controls | Reference |
| --- | --- | --- |
| `theme.kdl` | Colours, fonts, spacing, icons, how mail is displayed | [Theme reference](theme-reference.md) |
| `layout.kdl` | Panes, message list rows, toolbars, menus, keyboard shortcuts | [Layout reference](layout-reference.md) |

The configuration folder is `%APPDATA%\Nail` (type that into the Explorer
address bar to open it). If a file does not exist, Nail uses its built-in
defaults.

Start small: write only what you want to change, and Nail fills in the rest
from the defaults. For example, this `theme.kdl` changes the accent colour:

```kdl
theme "My theme" extends="default" {
    tokens {
        color "accent" "#c2185b"
    }
}
```

This `layout.kdl` binds `e` to archive in the message list:

```kdl
layout "My layout" extends="default" {
    keys context="list" {
        bind "e" "mail.archive"
    }
}
```

Save the file and Nail updates at once, with no restart.

If a file contains a mistake, Nail keeps using the last version that worked.
The status bar shows the file, line and column of the problem. Fix the file
and save it again.

To change how message rows look (for example, to show the sender and
subject on one line), see
[Row templates, parts and keyboard](templates-and-parts.md).

## Language

Nail follows the language of Windows or of your browser. You can also
choose a language when you start it, for example `nail serve --locale de`,
or by adding `?lang=de` to the address in the browser. Any text that has no
translation yet is shown in English.

Dates and times ("yesterday", "5 minutes ago", "5 Mar 2031, 14:30") are
written the way Nail's language writes them, not the way your browser
would: with Nail set to German in an English browser you see German dates,
and in a browser language Nail does not have yet (for example French) the
dates are English like the rest of Nail. Date fields you type into (for
example the snooze or the "send later" time) come from the browser or
Windows and follow their language.

## Where Nail keeps your data

| Folder | Contents |
| --- | --- |
| `%APPDATA%\Nail` | Your settings: `theme.kdl`, `layout.kdl`, `accounts.kdl`, `rules.kdl` (and their backups) |
| `%LOCALAPPDATA%\Nail` | Your mail, calendars and contacts: a local copy, so reading works offline; with encryption at rest also `vault.json` |

`nail serve --data-dir <folder> --config-dir <folder>` uses other folders,
which is handy for trying things out without touching your real setup.
To move Nail to another computer, make a [backup](backup-restore.md) and
restore it there.

## What Nail can do

- **Mail** with IMAP, POP3, Gmail and Microsoft Exchange accounts, a unified
  inbox and combined accounts, conversations, search, rules, snooze, junk
  filtering, undo send ([Reading and organizing mail](mail.md),
  [Writing mail](composer.md), [Accounts](accounts.md)).
- **Calendars, contacts and tasks**, invitations and free/busy
  ([Calendar, contacts and tasks](calendar-contacts-tasks.md)).
- **Signing and encryption** with OpenPGP, S/MIME and smartcards, and
  encryption at rest of everything stored ([Signing and encryption](security.md)).
- **Bringing your data along**: import from Thunderbird and standard files,
  export, encrypted backups ([Importing and exporting](import-export.md),
  [Backups and restoring](backup-restore.md)).
- **Your own look and layout** in `theme.kdl` and `layout.kdl`, changed live
  ([Customizing](customization.md)), plug-ins ([Plug-ins](plugins/README.md))
  and AI agents through MCP ([Connecting AI agents](mcp.md)).
- **Remote access** from your phone or another computer
  ([Remote access](remote-access.md)).

What is new in this version is in the [release notes](release-notes.md).
