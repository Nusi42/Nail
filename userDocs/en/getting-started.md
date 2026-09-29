# Getting started

Nail is an email client you can shape completely. Its look comes from one
file, `theme.kdl`, and its structure (which panes exist, where they sit,
which keys do what) comes from another, `layout.kdl`. You can change either
while Nail is running and see the result immediately.

This guide covers the current preview version (milestone M0). The preview
reads mail but does not send or change it. See
[What the preview can do](#what-the-preview-can-do).

## Start Nail

Nail runs in two ways, and both show the same interface:

- **As a desktop app.** Start *Nail* like any other Windows program.
- **In your browser.** Run `nail serve --open` in a terminal. Nail starts in
  the background and opens `http://127.0.0.1:4780/` in your browser. Only
  this computer can connect. Press `Ctrl+C` in the terminal to stop it.

### Try it with demo mail

To look around without an account, start Nail with the demo mailbox:

```sh
nail serve --seed demo --open
```

The demo contains about twenty messages in *INBOX*, *Sent* and *Archive*:
newsletters, a conversation thread, attachments, and a few deliberately
broken or hostile messages. They show how Nail protects you.

## Find your way around

The default window has three panes:

1. **Folders** (left): your accounts at the top, the folder tree below.
   The number next to a folder is its count of unread messages.
2. **Messages** (middle): the selected folder, newest first. Unread
   messages are bold.
3. **Message** (right): the selected message with its headers, attachments
   and text.

The status bar at the bottom shows whether Nail is up to date, the progress
of a running check for new mail, and the number of messages in the folder.

You can drag the dividers between the panes to resize them. When the window
is narrow, the panes become tabs.

### Keyboard shortcuts

| Key | Action |
| --- | --- |
| `j` / `↓` | Next message |
| `k` / `↑` | Previous message |
| `F5` | Check for new mail |
| `Ctrl+B` | Show or hide the folder pane |
| `g` then `r` | Show or hide the message pane |

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

## Where Nail keeps your data

| Folder | Contents |
| --- | --- |
| `%APPDATA%\Nail` | Your settings: `theme.kdl`, `layout.kdl` |
| `%LOCALAPPDATA%\Nail` | Your mail: a local copy of your folders and messages, so reading works offline |

`nail serve --data-dir <folder> --config-dir <folder>` uses other folders,
which is handy for trying things out without touching your real setup.

## What the preview can do

The M0 preview can:

- show the demo mailbox, or an IMAP mailbox that Nail has been set up to
  read (setting up your own account comes in the next version);
- read messages safely, including HTML mail, attachments listed with their
  sizes, and mail in older character sets;
- be reshaped completely with `theme.kdl` and `layout.kdl`, live.

The next version adds adding your own accounts, replying and writing mail,
archiving, deleting and flagging, search, and conversation view. Until then,
the toolbar buttons for these actions are shown but disabled.
