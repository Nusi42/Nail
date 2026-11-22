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

## Saving your changes

Changes in a section take effect when you save them. As soon as you change
something, a bar appears at the bottom of the section: **Unsaved changes to
“…”** with **Save** and **Discard**. Save keeps every change of the section;
Discard puts the saved values back.

If you open another section or close Settings with unsaved changes, Nail
asks first: **Save changes to “…”?** with **Save**, **Don't save** and
**Cancel** (stay in the section). The same question comes when you pick
another item inside a section while the shown one has unsaved changes —
another account, virtual account, folder, MCP client, remote user or key.
When a change cannot be saved yet (a field shows what is wrong), **Save**
in the bar is disabled and the question offers **Discard changes** or
**Keep editing**.

This holds for every setting, switches and choices included: turning
notifications off, choosing a folder's download policy, granting an MCP
client a scope or changing the tray behaviour all wait for **Save**. If
saving fails, the section keeps your changes and the bar, and shows why.

Actions are not settings and happen when you choose them, after a
question where they cannot be undone: **Remove account…**, **Delete
virtual account…**, **Reset the filter…**, **Revoke…**, **Delete key…**,
turning encryption at rest on or off, and the remote access actions
(delete a user or passkey, revoke a device, make a new certificate). The
question names what happens; for anything that deletes, the safe button
(**Cancel**) has the focus and Enter does not confirm.

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
with, found by name or address — opened while the field is empty, the list
shows the addresses you use most (your own addresses are left out), except
in fields for your own addresses (a vacation reply's sender and aliases),
which offer only the account's own addresses; tags
offer the ones in use and **Create tag "…"** for a new one.

You can still type any value yourself; the list only helps to find one.
For a field that holds an account or a folder, a typed name that matches
exactly one choice is taken as that choice when you leave the field. A
field that selects which account a setting is for only ever changes to an
account from its list.

**Several values.** Some fields hold a list (several addresses, accounts or
folders). Each value is shown as a chip before the field, with **×** to
remove it. Picking a choice adds it, and the list stays open for the next
one; a chosen choice is marked with a check and a soft background, and
picking it again removes it. Typed text becomes
a value with **Enter**, a comma or a semicolon, or when you leave the field
(for accounts and folders only a name that matches a choice); pasting
several addresses separated by commas adds them all. **Backspace** in the
empty field removes the last value. Screen readers hear what the field
holds when they reach it, and each value added or removed is announced.

Keyboard: **Down** / **Up** move through the list, **Home** / **End** jump
to its ends while it shows every choice, **Right** opens a folder with
subfolders (or moves into it), **Left** closes it (or moves to its parent),
**Enter** picks the marked choice, **Escape** or **Tab** closes the list
without changing the field. A click outside the list closes it, too.

These fields use it: in **Accounts**, the folders of a merged folder of a
virtual account and an identity's reply-to address; in **Mail & junk →
Synchronization**, the account and folder whose synchronization you set; in
**Mail & junk → Junk mail**, the account whose filter you set; in
**Notifications**, the folder you add an override for; in **Server rules &
vacation**, the account, the sender of the vacation reply and **Also my
addresses** (several); in **Privacy & data protection**, the accounts with
searchable decrypted mail (several); in **Keys & encryption**, the address
of **Find online**; in **Agents (MCP)**, a client's accounts and hidden
folders (several); in **Import** and **Export**, the account, the target
folder and the folders to export (several); in **Automatic replies** of
Exchange accounts, the account. The rules editor uses the same lists
([Rules](rules.md)).

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
