# Layout reference (`layout.kdl`)

What Nail shows and where — panes, widgets, message list rows, toolbars,
menus and keyboard shortcuts — comes from one file, `layout.kdl`, in your
Nail configuration folder. How things look is set in
[`theme.kdl`](theme-reference.md).

This page covers what works in the current version (M1).

## The short version

Create `layout.kdl` in your configuration folder and change only what you
want; everything else comes from the built-in default layout:

```kdl
layout "My layout" extends="default" {
    keys context="list" {
        bind "e" "mail.archive"
        unbind "k"
    }
}
```

Save the file and Nail updates immediately. If the file has a mistake, Nail
keeps the last working layout and tells you the line and column of the
problem.

The complete default layout ships with Nail as `defaults/layout.kdl`.

## File structure

```kdl
layout "<name>" extends="default" {
    breakpoint min-width=900 {
        window "main" { … }
    }
    window "composer" { … }
    actions { action "<id>" … }
    keys context="<context>" { bind "<key>" "<action>" }
    toolbar "<name>" { item "<action>" }
    menu "<name>" { item "<action>" }
}
```

`extends="default"` starts from the default layout. Without it, the file
must be complete; it needs at least a `main` window.

The file format is [KDL](https://kdl.dev) version 2. A long node can
continue on the next line after a `\`.

## Windows and breakpoints

A `window` holds exactly one root node: a container or a widget. The main
window is called `main`.

`breakpoint` chooses a different tree depending on the window width (in CSS
pixels, both bounds inclusive). The first breakpoint that fits wins; a
`window` outside any breakpoint fits every width.

```kdl
breakpoint min-width=900 {
    window "main" { split "horizontal" { folder-tree; message-list; message-view } }
}
breakpoint max-width=899 {
    window "main" { tabs { folder-tree; message-list; message-view } }
}
```

If your file defines a window (in any breakpoint), it replaces all of the
default's trees for that window.

Besides `main`, the default layout has these windows. They open over the
main window (Escape or the close button closes them), and their content is
laid out like any other window:

| Window | Opened by | Default content |
| --- | --- | --- |
| `composer` | `mail.compose`, `mail.reply`, `mail.reply-all`, `mail.forward` | `composer` with `security-composer` below it |
| `outbox` | `outbox.open` | `outbox` |
| `move` | `mail.move` | `folder-picker` |
| `account-setup` | `account.add` | `account-setup` |
| `rules` | `rules.open`, `rules.create-from-message` | `rule-manager` |
| `settings` | `settings.open` | `tabs` with `account-settings`, `virtual-account-editor`, `mcp-clients`, `security-keys`, `security-identities`, `security-settings`, `security-autocrypt` and a "Customize" tab (a `toolbar` and `config-errors`) |

```kdl
window "settings" {
    tabs {
        account-settings label="@settings.tab.accounts"
        virtual-account-editor label="@settings.tab.virtual-accounts"
    }
}
```

## Containers

Every node can have an `id` (lowercase words, unique within one window
tree) and a `label`.

### `split`

Panes next to each other (`"horizontal"`, the default) or on top of each
other (`"vertical"`), with draggable dividers.

```kdl
split "horizontal" sizes="240px 1fr 1.4fr" \
    min-sizes="160px 280px 320px" max-sizes="480px none none" \
    collapsible="true false true" {
    folder-tree
    message-list
    message-view
}
```

| Property | Meaning |
| --- | --- |
| `sizes` | Starting size of each pane: `px`, `%`, `fr` (share of the free space), or `auto` (as wide as its content, like the navigation rail). One value per pane. |
| `min-sizes`, `max-sizes` | Limits per pane; `none` for no limit. |
| `collapsible` | `true` or `false` per pane. |
| `resizable` | `#false` to lock the dividers. |

### `stack`

Children from top to bottom. `sizes` works like for `split`; `auto` means
"as tall as its content". Without `sizes`, bars (toolbar, status bar,
account switcher, text) take their natural height and everything else
shares the rest.

```kdl
stack sizes="auto 1fr" {
    toolbar toolbar="reader"
    message-view
}
```

### `tabs`

One child at a time with a tab strip. `active` is the tab shown first
(counting from 0). Tab titles come from the children's `label`.

```kdl
tabs active=1 {
    folder-tree label="@layout.pane.folders"
    message-list label="@layout.pane.messages"
}
```

## Widgets

A widget is a node named after its kind. Its properties are its options.

| Widget | Options |
| --- | --- |
| `account-switcher` | `show-status` (`#true`/`#false`): a dot per account showing its connection state |
| `folder-tree` | `show-unread` (`#true`/`#false`), `show-account-badges` (`#true`/`#false`), `sections` (any of `favorites accounts smart-folders`, in the order shown; default all three), `favorites` (the unified folders under Favorites: any of `all-inboxes unread flagged snoozed drafts sent outbox`; default `all-inboxes flagged snoozed drafts`), `collapsible` (`#true`/`#false`: sections fold away; Nail remembers which), `account-colors` (`#true`/`#false`: a colour dot per account), `compose` (`#true`/`#false`, default `#true`: the **New message** button on top). The grouped pane (New message, Favorites, one section per account, smart folders) appears as soon as one of `sections`, `favorites`, `collapsible`, `account-colors` or `compose` is set; without them the older single-account tree is shown |
| `list-header` | above the message list: `search` and `hint` (the search field with the key that focuses it — the default layout binds `/` to `search.focus`), `title` and `counts` (the folder's name with its counts, e.g. "All inboxes · 102 unread · 4 accounts"), `chips` (filter chips: any of `all unread flagged attachments`; they search with `is:unread`, `is:flagged`, `has:attachment` inside the shown folder) — each `#true`/`#false` except `chips` |
| `nav-rail` | the navigation rail (see [Views and the navigation rail](#views-and-the-navigation-rail)): `show-labels` (`#true`/`#false`, default `#false`: visible labels under the icons; screen readers always hear them), `show-logo` (`#true`/`#false`, default `#true`) |
| `view-host` | — (shows the active view, remembers the last one; see [Views and the navigation rail](#views-and-the-navigation-rail)) |
| `settings-page` | `search` (`#true`/`#false`): the settings page of the `settings-page` section (see [The settings page](#the-settings-page)) |
| `message-list` | `page-size` (number), `group-by` (`none`, `date`, `sender`), `threads` (`off`, `collapsed`: one row per conversation, `expanded`: one row per message), `follow-search` (`#true`/`#false`: show search results while a search is active), `columns` (column order, written back by Nail); `row` templates |
| `message-view` | `headers` (e.g. `"from to cc date"`), `mark-read` (`#true`/`#false`: mark a message read when it is shown); `row` templates |
| `thread-view` | `expand` (`latest`, `unread`, `all`, `none`: which messages of the conversation start opened), `hide-single` (`#true`/`#false`: hide for messages without replies) |
| `search-bar` | `scope` (`account`, `folder`, `all`), `hints` (`#true`/`#false`: suggest search fields while typing), `live-check` (`#true`/`#false`: point out mistakes while typing) |
| `composer` | `send-later` (`#true`/`#false`: offer "Send later") |
| `outbox` | — |
| `folder-picker` | — (the "Move to folder" list) |
| `account-setup` | — (the "Add account" assistant) |
| `account-settings` | — |
| `virtual-account-editor` | — |
| `rule-manager` | — (the filter rules: list, editor, test and run history; see [Filter rules](rules.md)) |
| `mcp-clients` | — (MCP clients, their access and the activity log; see [Connecting AI agents](mcp.md)) |
| `mcp-context-indicator` | — (the notice shown while an agent reads what is on screen) |
| `security-keys` | — (keys and certificates: import, create, export, trust, find online; see [Signing and encryption](security.md)) |
| `security-identities` | — (signing and encryption defaults per sender address) |
| `security-settings` | — (encryption at rest, online key lookup, searching encrypted mail) |
| `security-autocrypt` | — (Autocrypt setup message: send to your other devices, import) |
| `security-message` | — (the signature / encryption badge and the sender check badge of the shown message, each with details) |
| `security-warnings` | — (warnings about deceptive links and risky attachments of the shown message) |
| `security-composer` | — (Sign and Encrypt switches with the key status of every recipient) |
| `config-errors` | `hide-when-valid` (`#true`/`#false`): show nothing while every config file is valid (see [Customizing](customization.md#all-configuration-errors-at-once)) |
| `toolbar-editor`, `key-binding-editor`, `column-editor` | — (the Customize editors for toolbars, shortcuts and list columns; see [Customizing](customization.md#editing-toolbars-shortcuts-and-columns)) |
| `sieve-status` | — (server-side rules: ManageSieve per account, the script's state, where each rule runs; see [Server-side rules and vacation replies](server-side-rules.md)) |
| `vacation-editor` | — (the vacation reply of an account) |
| `junk-settings` | — (Nail's junk filter per account and its training; see [Junk mail, snooze and notifications](junk-snooze-notifications.md#junk-mail)) |
| `junk-verdict` | — (in the reader: "This message is junk" with **Not junk**) |
| `snooze-list` | — (the snoozed messages with **Wake up now**) |
| `notification-settings` | — (new-mail notifications, per account and folder, quiet hours, a test notification, allowing them in the browser; see [Junk mail, snooze and notifications](junk-snooze-notifications.md#new-mail-notifications)) |
| `sync-folder-policies` | — (what each folder downloads; see [Reading and organizing mail](mail.md#messages-that-are-not-downloaded-yet)) |
| `sync-message-body` | — (in the reader: "Only the headers are stored" with **Download full message**) |
| `smart-folders` | `editable` (`#true`/`#false`): offer "Save current search" and remove buttons (see [Customizing](customization.md#smart-folders-saved-searches)) |
| `toolbar` | `toolbar`: the name of a `toolbar` definition (required) |
| `status-bar` | `items`: any of `sync-progress`, `account-status`, `ops` (changes being saved), `send-status` (undo send), `folder-counts` |
| `text` | `text`: literal text or `@message.id` |
| `spacer` | — |
| `plugin-panel` | the panel as argument: `plugin-panel "<plugin-id>.<panel>"` (or `panel="…"`); see [Plug-ins](plugins/README.md) |
| `plugin-manager` | — (the installed plug-ins; the Plug-ins tab of the settings window) |
| `import-export` | `show-jobs` (`#true`/`#false`, default `#true`): the recent imports and exports with their reports (see [Importing and exporting](import-export.md)) |
| `backup-restore` | `show-jobs` (`#true`/`#false`, default `#true`): the recent backups, checks and restores (see [Backups and restoring](backup-restore.md)) |
| `diagnostics` | — (the diagnostics bundle for support; see [Diagnostics bundle](diagnostics.md)) |
| `security-recovery-key` | — (the recovery key of encrypted data: create, check, replace, remove; see [Recovery key](recovery-key.md)) |
| `security-smartcards` | — (PKCS#11 modules, the PIN cache, cards and their keys; see [Smartcards](smartcards.md)) |

The calendar, contact and task widgets (`calendar`, `agenda`,
`mini-month`, `task-list`, `contact-list`, `contact-card`, `calendar-list`)
and their options are described in
[Calendar, contacts and tasks](calendar-contacts-tasks.md#layout-options); the invitation,
free/busy, Gmail label and category widgets in
[Invitations, scheduling and Gmail labels](invitations-and-gmail-labels.md#layout-options); the desktop app's
`update-settings`, `desktop-settings` and `default-apps` (and the `system.*`
actions) in [The desktop app on Windows](desktop-app.md#layout-options). Plug-in panels are written `plugin-panel "<plugin-id>.<panel>"`;
the older form `plugin.<plugin-id>.<panel>` is refused with a hint to it.

```kdl
stack sizes="auto auto 1fr" {
    toolbar toolbar="reader"
    thread-view expand="unread"
    message-view headers="from date" mark-read=#false
}
```

### Row templates

`message-list` rows are built from templates, one `row` per line:

```kdl
message-list {
    row "{from.name}  {date | relative}"
    row "{subject}"
    row "{preview}"
}
```

`{…}` inserts a field, optionally passed through filters with `|`.
Braces must be balanced.

## Labels and languages

A `label` is either literal text (`label="Mail"`) or a message id from
Nail's translations, written with `@` (`label="@layout.pane.messages"`). Use
message ids so the label follows your language.

## Actions

Every command has an id such as `mail.reply` or `view.toggle`. Toolbars,
menus and keys refer to these ids. The `actions` section sets how an action
is shown, and can define your own aliases:

```kdl
actions {
    action "mail.reply" label="@mail.action.reply" icon="reply"
    action "goto.inbox" alias-of="nav.goto-folder" "Inbox" label="Inbox"
}
```

| Property | Meaning |
| --- | --- |
| `label` | Text or `@message.id` |
| `icon` | An icon name from the theme |
| `alias-of` | Makes this action run another one; further values after the id are its arguments |

In a sparse file only the properties you write change.

Actions in the default layout:

| Action | What it does |
| --- | --- |
| `mail.next`, `mail.previous` | Select the next / previous message |
| `mail.compose` | New message (continues an unfinished one) |
| `mail.reply`, `mail.reply-all`, `mail.forward` | Answer or forward the selected message |
| `mail.archive` | Move the selected message to the account's Archive |
| `mail.move` | Open "Move to folder" |
| `mail.move-to` | Move to a folder; argument: a folder role (`"archive"`, `"trash"`, …) or folder id |
| `mail.delete`, `mail.delete-permanently` | Move to Trash / delete for good |
| `mail.toggle-read`, `mail.toggle-flag` | Mark read or unread, flag or unflag |
| `composer.send`, `composer.save`, `composer.discard` | In the composer: send, save the draft, throw the message away |
| `composer.set-mode` | Switch the composer's format; argument `"plain"`, `"html"` or `"markdown"` |
| `composer.toggle-preview`, `composer.toggle-source`, `composer.attach` | Show or hide the Markdown preview, show or hide the HTML source, attach a file |
| `rules.open`, `rules.create-from-message` | Open the filter rules; start a rule from the selected message |
| `inspector.toggle`, `customize.merge`, `customize.errors` | Inspector on or off; the merge dialog; the list of configuration errors (see [Customizing](customization.md)) |
| `mcp.indicator.dismiss` | Hide the "agent is reading" notice |
| `security.autocrypt.import` | Import the selected Autocrypt setup message (asks for its setup code; see [Signing and encryption](security.md)) |
| `outbox.open`, `outbox.undo` | Show the outbox; take back the message being sent |
| `search.focus`, `search.clear` | Jump to the search bar; end the search |
| `account.add`, `settings.open` | Add an account; open the settings |
| `window.open`, `window.close` | Open a window by name (argument); close the topmost window |
| `view.toggle` | Show or hide a pane; argument: its `id`, e.g. `"sidebar"` |
| `sync.now` | Check for new mail |
| `menu.context` | Open the context menu of the focused item (default keys `shift+f10` and `contextmenu`, the menu key) |
| `junk.mark`, `junk.not-junk` | Mark the selected message as junk / not junk (trains the junk filter, moves the message) |
| `snooze.open`, `snooze.list` | Snooze the selected message; show the snoozed messages |
| `sync.download-message` | Download the full message when only its headers are stored |
| `sync.folder-policy` | What a folder downloads (in a folder's menu: that folder; elsewhere the open folder) |
| `palette.open` | Open (or close) the command palette (default `ctrl+k` and `ctrl+shift+p`, also over dialogs; see [Command palette](command-palette.md)) |
| `print.message`, `print.thread`, `print.event`, `print.agenda`, `print.save-pdf` | Print the selected message, its conversation, the selected event or an agenda; save the message as PDF (default `ctrl+p` in the list and the reader; see [Printing](printing.md)) |
| `import.open`, `export.open` | The import and export assistants |
| `backup.create`, `backup.restore` | Back up Nail; restore from a backup |
| `diagnostics.open` | Create a diagnostics bundle |

```kdl
actions {
    action "mail.spam" alias-of="mail.move-to" "junk" label="Spam" icon="junk"
}
keys {
    bind "shift+j" "mail.spam"
}
```

## Keys

```kdl
keys context="list" {
    bind "j" "mail.next"
    bind "g i" "nav.goto-folder \"Inbox\""
    bind "ctrl+shift+m" "view.toggle" "sidebar"
    unbind "k"
}
```

- `context` is where the keys work: `list` (message list), `reader`
  (message and conversation), `folders`, `search` (search bar), `composer`,
  `calendar` (calendar and agenda), `contacts`, `tasks`, `window` (any
  window over the main window), `menu` (an open context menu),
  `plugin-dialog` (a plug-in's dialog) and `plugin-veto` (a message a
  plug-in held back). Without `context` they work everywhere in the main
  window (`global`).
- While you type in a text field, only combinations with `Ctrl`, `Alt` or
  `Meta` and `escape` work, and only in that field's contexts (for example
  `ctrl+enter` in the composer). Global shortcuts do not work inside
  windows such as the composer.
- Keys are written like `j`, `enter`, `arrowdown`, `f5`, `delete`.
  Combinations join keys with `+` (`ctrl+b`); sequences separate steps with
  a space (`g i` = press `g`, then `i`).
- The action may include arguments inside the string (quoted text or plain
  words and numbers), or as further values after it.
- Binding a key again in the same context replaces the earlier binding.
  `unbind` removes a binding, including one from the default layout.

## Toolbars

```kdl
toolbar "reader" {
    item "mail.reply"
    item "mail.forward" label="Pass on"
    spacer
    item "mail.archive"
    separator
    item "mail.toggle-flag"
}
```

`item` shows an action (with optional `label` and `icon` overrides),
`spacer` pushes the rest to the end, `separator` draws a line. A `toolbar`
widget shows a toolbar by name. Defining a toolbar with an existing name
replaces it.

### Ranked items and the "More" menu

When a toolbar is too narrow for its items, they move into a **More** menu
at its end instead of wrapping onto a second line:

```kdl
toolbar "reader" hide-disabled=#true {
    item "mail.reply" rank=1 display="icon-label"
    item "mail.reply-all" rank=2 display="icon-label"
    item "mail.forward" rank=3 display="icon-label"
    separator
    item "mail.archive" rank=4 display="icon"
    item "mail.delete" rank=4 display="icon"
    spacer
    more {
        header "@toolbar.group.message"
        item "mail.toggle-read"
        item "junk.mark"
        separator
        header "@toolbar.group.turn-into"
        item "calendar.event-from-message"
        item "tasks.from-message"
    }
}
```

- `rank=` decides what moves first: items without a rank go first (from the
  end of the bar), then the highest rank; `rank=1` stays longest.
- `display=` shows the `icon`, the `label` or both (`icon-label`, the
  default). Icon-only buttons keep their label as tooltip (with the key, for
  example "Archive (E)") and for screen readers. The item with `rank=1` gets
  the primary look (the reader's **Reply** in the default layout).
- `more { … }` is the **More** menu with its own entries (`item`,
  `separator`, `submenu`, and `header "<label>"` for a group heading); items
  that do not fit are added below them. A toolbar has at most one `more`;
  without one, Nail adds it when something does not fit. The menu shows the
  key bound to each action.
- `hide-disabled=#true` hides actions that cannot run right now (for
  example Gmail labels on a non-Gmail message) instead of showing them
  dimmed — in the **More** menu too.
- `overflow="wrap"` keeps the old behaviour: items wrap onto further lines.

## Menus

```kdl
menu "context:message-list.row" {
    item "mail.reply"
    separator
    submenu "@menu.more" {
        item "mail.toggle-flag"
    }
}
```

`"app"` is the application menu; `"context:<part>"` is the right-click menu
of a part. Defining a menu with an existing name replaces it. `header
"<label>"` adds a group heading; a heading without an entry below it is
left out. Parts with a context menu:

| Part | Where | Shortcuts shown from context |
| --- | --- | --- |
| `message-list.row` | A message in the list (it becomes the selected message) | `list`, then global |
| `thread-view.message` | A message of the conversation above the reader | `reader` |
| `message-view` | The shown message | `reader` |
| `message-view.attachment` | An attachment of the shown message | `reader` |
| `folder-tree.folder` | A folder (the folder is not opened; its actions use it) | global |
| `folder-tree.account` | An account's heading in the folder pane | global |
| `composer` | The composer outside its text fields | `composer` |
| `calendar-list.calendar` | A calendar in the calendar list (its "…" button) | `calendar` |

Text fields keep the usual Cut / Copy / Paste menu. Each entry shows the
action's label and icon (or the item's `label` / `icon`) and the key bound
to it. Separators at the start, at the end or twice in a row are dropped,
and so are empty submenus. The menu opens on a right-click and, from the
keyboard, with the action `menu.context` (Shift+F10 and the menu key by
default); see [Accessibility](accessibility.md#context-menus) for its keys.

## Views and the navigation rail

Mail, Calendar, Contacts, Tasks and Settings are **views**. Each view shows
one of the layout's windows; the `view-host` widget shows the active one,
and the `nav-rail` widget switches between them:

```kdl
layout "With a rail" extends="default" {
    window "main" {
        split "horizontal" id="shell" sizes="auto 1fr" resizable=#false {
            nav-rail id="rail"
            view-host id="views"
        }
    }
    window "mail" {
        split "horizontal" id="panes" sizes="240px 1fr 1.4fr" {
            folder-tree id="folders" sections="favorites accounts smart-folders"
            stack id="list-pane" sizes="auto 1fr" {
                list-header id="list-header" chips="all unread flagged attachments"
                message-list id="list"
            }
            message-view id="message"
        }
    }
    views default="mail" {
        view "mail"     label="@view.mail" icon="mail" window="mail" badge="unread"
        view "calendar" label="@view.calendar" icon="calendar" window="calendar"
        view "contacts" label="@view.contacts" icon="users" window="contacts"
        view "tasks"    label="@view.tasks" icon="square-check-big" window="tasks"
        view "settings" label="@view.settings" icon="settings" window="settings"
    }
}
```

```kdl
rail {
    view "mail"
    view "calendar"
    view "contacts"
    view "tasks"
    spacer
    item "agents.open-center" icon="shield-check" badge="confirmations"
    view "settings"
}
```

- `view "<id>"` needs `window=` (a window of the layout that does not itself
  contain a `view-host`); `label`, `icon` (a Lucide name) and `badge` are
  optional. `default=` names the view shown at start (else the first one).
- `badge="unread"` counts unread mail in the inboxes, `badge="confirmations"`
  the changes by agents or plug-ins waiting for your approval.
- The rail lists views and actions (`item`, with optional `label`, `icon`,
  `badge`), `spacer` (the rest goes to the bottom) and `separator`.
- The action `view.show "<view>"` switches views; the default layout binds
  `ctrl+1` … `ctrl+4` (and `g m`) through the aliases `view.mail`,
  `view.calendar`, `view.contacts`, `view.tasks` and `view.settings`;
  `view.next` / `view.previous` step through the rail. Without a
  `view-host` in the main window, switching to a view opens its window
  instead. The default layout's `pim.calendar.open`, `pim.contacts.open`,
  `pim.tasks.open` and `settings.open` show their view, too.
- The rail's badges: `unread` is the unread mail in the inboxes of all
  accounts, `confirmations` the changes waiting for your approval; numbers
  above 99 show as "99+".
- The view host remembers the last view and shows it again at the next
  start. Every view stays loaded once you opened it, so switching back keeps
  the selected message, scroll positions and the calendar's dates.
- Each view has its own keyboard context: `calendar`, `contacts`, `tasks`,
  else the global keys (with the mail view's `list` / `reader` contexts
  inside it). The keys of a hidden view never fire. On the rail, Up / Down /
  Home / End move between its buttons.
- A `views`, `rail` or `settings-page` section in your file replaces the
  default's as a whole; files without them keep the defaults.

### Layouts from before the rail

A `layout.kdl` written for an older Nail keeps working: its own
`window "main"` still shows, the views, rail and settings page come from the
default, and switching to a view (Ctrl+2, the command palette) opens the
view's window over the main window. When Nail starts and your file defines
its own main window without a `view-host`, it offers the new default once
(**Review the new layout** opens the merge dialog with it, see
[Customizing](customization.md#the-new-default-layout-for-layouts-of-your-own)),
where you pick what to take over; nothing changes until you apply it.

## The settings page

The `settings-page` section groups the settings widgets into sections; the
`settings-page` widget shows them with a navigation and a search over every
setting's label:

```kdl
settings-page {
    group "general" label="@settings.group.general" {
        section "accounts" label="@settings.section.accounts" icon="user-round" {
            account-settings
            virtual-account-editor
        }
    }
    group "look-and-feel" label="@settings.group.look-and-feel" {
        section "appearance" label="@settings.section.appearance" icon="palette" keywords="theme dark" {
            appearance-settings
        }
    }
}
```

Group and section ids are unique on the page, and so are the `id`s of the
widgets in it. A section with several widgets shows them top to bottom;
`keywords=` adds search words. The default layout has the groups General,
Look & feel, Security and Extensions.

## Error messages

A mistake is reported with the file, line and column, for example:

```
layout.kdl:8:17: unknown option `show-unreed` for `folder-tree` (options: show-unread, show-account-badges)
```

Until you fix it, Nail keeps using the last layout that worked.
