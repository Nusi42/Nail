# Settings

All of Nail's settings are on one page with a navigation on the left and
the chosen section on the right. Open it from the rail (the gear at the
bottom), with **Ctrl+,**, or from the command palette (**Ctrl+K**, then
"Settings" or "Search settings").

Nail remembers the section you looked at last and opens there next time.

## Sections

The sections are grouped like this in the default layout:

| Group       | Section                   | What you find there                                                                                             |
| ----------- | ------------------------- | --------------------------------------------------------------------------------------------------------------- |
| General     | Accounts                  | Your accounts (servers, sign-in, identities, signatures, sender checks) and virtual accounts                    |
| General     | Mail & junk               | What each folder downloads (synchronization) and the junk filter                                                |
| General     | Calendar & contacts       | Your calendars and task lists, calendar sources and subscriptions, importing calendar and contact files         |
| General     | Server rules & vacation   | Rules that run on your mail server (Sieve), Exchange inbox rules, the vacation reply                            |
| General     | Notifications             | New-mail notifications, quiet hours, per account and folder                                                     |
| General     | Data & backup             | Import and export, backups and restoring, the diagnostics bundle                                                |
| General     | Desktop app               | Updates, tray and starting with Windows, default apps                                                           |
| Look & feel | Appearance                | Themes, Follow Windows, density, text size, corners and accent colour                                           |
| Look & feel | Layout & toolbars         | The toolbar and list-column editors and configuration errors                                                    |
| Look & feel | Keyboard shortcuts        | Every shortcut, by where it works                                                                               |
| Security    | Keys & encryption         | Keys and certificates, signing and encryption per address, Autocrypt, smartcards                                |
| Security    | Privacy & data protection | Encryption at rest, the recovery key, searching encrypted mail, finding keys online, remote content you allowed |
| Security    | Remote access             | Serving Nail to other devices, users, devices and sessions, the security log                                    |
| Extensions  | Agents (MCP)              | AI agents connected through MCP and the confirmations they wait for                                             |
| Extensions  | Plug-ins                  | Installed plug-ins, their permissions, settings and logs                                                        |

Each part of a section sits on its own card; inside a card, the name and
description of a setting are on the left and its control on the right.

Which sections exist, their order, names and icons come from the
`settings-page` section of `layout.kdl` (see
[The settings page](layout-reference.md#the-settings-page)), so your
layout can move a setting to another section or add your own section.

In a browser (`nail serve` or the remote UI) the desktop-only cards —
Updates, Tray and startup, Default apps — say that the setting is available
in the Nail desktop app.

### Where the old tabs went

Earlier versions of Nail showed the settings as tabs. Every tab is now in a section:

| Old tab                                                              | Section                   |
| -------------------------------------------------------------------- | ------------------------- |
| Accounts, Virtual accounts                                           | Accounts                  |
| Synchronization, Junk mail                                           | Mail & junk               |
| Server rules, Exchange rules, Vacation reply                         | Server rules & vacation   |
| Notifications                                                        | Notifications             |
| Import and export, Backups, Diagnostics                              | Data & backup             |
| Updates, Tray and startup, Default apps                              | Desktop app               |
| Appearance                                                           | Appearance                |
| Customize (toolbars, columns, errors)                                | Layout & toolbars         |
| Customize → Keys                                                     | Keyboard shortcuts        |
| Keys and certificates, Signing and encryption, Autocrypt, Smartcards | Keys & encryption         |
| Data protection, Recovery key, Privacy                               | Privacy & data protection |
| Remote access, Remote users, Devices, Security log                   | Remote access             |
| MCP clients, Confirmations                                           | Agents (MCP)              |
| Plug-ins                                                             | Plug-ins                  |

## Searching

Type into **Search settings** at the top of the navigation (or press the
shortcut of "Search settings" from anywhere). Nail looks through every
section and group name and the name of every setting, plus common other
words (for example "spam", "out of office", "dark mode" or "pgp"). Upper
and lower case and accents do not matter, and every word you type has to
appear somewhere in the section.

The results replace the navigation while you search:

- each matching section, best match first, with the matching words
  highlighted;
- below a section, the settings that matched. Choosing one opens the
  section, scrolls to the setting, marks it for a moment and puts the
  keyboard focus on its control.

Keyboard: **Enter** opens the first result, **Down** moves into the
results, **Escape** clears the search (a second **Escape** closes the
window when Settings is one). The **×** button clears the search too.

## Choosing a folder, an account or an address

Fields that name a folder, an account, a tag, an address, a mailing list or
a header name have a list to choose from: the **▾** button beside the field
(or **Down** in the field) shows every choice, and typing searches them.
Folders are a tree: each account with its folders (Inbox, Sent, Archive and
the other special folders first, with their purpose beside a folder whose
name differs); addresses come from your contacts and the people you write
with, found by name or address; tags offer the ones in use and **Create
tag "…"** for a new one.

You can still type any value yourself; the list only helps to find one.
For a field that holds an account or a folder, a typed name that matches
exactly one choice is taken as that choice when you leave the field.

Keyboard: **Down** / **Up** move through the list, **Home** / **End** jump
to its ends while it shows every choice, **Right** opens a folder with
subfolders (or moves into it), **Left** closes it (or moves to its parent),
**Enter** picks the marked choice, **Escape** or **Tab** closes the list
without changing the field. A click outside the list closes it, too.

These fields use it: in **Accounts**, the folders of a merged folder of a
virtual account and an identity's reply-to address; in **Mail & junk →
Synchronization**, the account and folder whose synchronization you set; in
**Notifications**, the folder you add an override for; in **Server rules &
vacation**, the sender of the vacation reply. The rules editor uses the same
lists ([Rules](rules.md)).

## Links to a section

The action `settings.section "<id>"` opens the page at a section. You can
use it in a toolbar, a menu or a key binding of `layout.kdl`:

```kdl
keys {
    bind "ctrl+alt+a" "settings.section" "appearance"
}
```

The id can be a section id (`appearance`, `keys`, …), the id of a widget
on the page, a widget kind (`junk-settings`), or the id of one of the old
tabs (`security-keys`, `vacation`, `sync-policies`, …). For a widget or an
old tab, Nail also scrolls to that part of the section. `settings.search`
opens the page with the search box focused.
