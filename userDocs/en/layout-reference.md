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
| `composer` | `mail.compose`, `mail.reply`, `mail.reply-all`, `mail.forward` | `composer` |
| `outbox` | `outbox.open` | `outbox` |
| `move` | `mail.move` | `folder-picker` |
| `account-setup` | `account.add` | `account-setup` |
| `settings` | `settings.open` | `tabs` with `account-settings` and `virtual-account-editor` |

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
| `sizes` | Starting size of each pane: `px`, `%`, or `fr` (share of the free space). One value per pane. |
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
| `folder-tree` | `show-unread` (`#true`/`#false`), `show-account-badges` (`#true`/`#false`) |
| `message-list` | `page-size` (number), `group-by` (`none`, `date`, `sender`), `threads` (`off`, `collapsed`: one row per conversation, `expanded`: one row per message), `follow-search` (`#true`/`#false`: show search results while a search is active); `row` templates |
| `message-view` | `headers` (e.g. `"from to cc date"`), `mark-read` (`#true`/`#false`: mark a message read when it is shown); `row` templates |
| `thread-view` | `expand` (`latest`, `unread`, `all`, `none`: which messages of the conversation start opened), `hide-single` (`#true`/`#false`: hide for messages without replies) |
| `search-bar` | `scope` (`account`, `folder`, `all`), `hints` (`#true`/`#false`: suggest search fields while typing), `live-check` (`#true`/`#false`: point out mistakes while typing) |
| `composer` | `send-later` (`#true`/`#false`: offer "Send later") |
| `outbox` | — |
| `folder-picker` | — (the "Move to folder" list) |
| `account-setup` | — (the "Add account" assistant) |
| `account-settings` | — |
| `virtual-account-editor` | — |
| `toolbar` | `toolbar`: the name of a `toolbar` definition (required) |
| `status-bar` | `items`: any of `sync-progress`, `account-status`, `ops` (changes being saved), `send-status` (undo send), `folder-counts` |
| `text` | `text`: literal text or `@message.id` |
| `spacer` | — |

The widgets `calendar`, `agenda`, `mini-month`, `task-list`,
`contact-list` and `contact-card` are accepted in the file but not
available yet. Plug-in widgets are written `plugin.<plugin-id>.<name>`.

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
| `outbox.open`, `outbox.undo` | Show the outbox; take back the message being sent |
| `search.focus`, `search.clear` | Jump to the search bar; end the search |
| `account.add`, `settings.open` | Add an account; open the settings |
| `window.open`, `window.close` | Open a window by name (argument); close the topmost window |
| `view.toggle` | Show or hide a pane; argument: its `id`, e.g. `"sidebar"` |
| `sync.now` | Check for new mail |

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
  `window` (any window over the main window). Without `context` they work
  everywhere in the main window (`global`).
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
of a part (see the part names in the [theme reference](theme-reference.md)).
Defining a menu with an existing name replaces it.

## Error messages

A mistake is reported with the file, line and column, for example:

```
layout.kdl:8:17: unknown option `show-unreed` for `folder-tree` (options: show-unread, show-account-badges)
```

Until you fix it, Nail keeps using the last layout that worked.
