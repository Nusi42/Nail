# Layout reference (`layout.kdl`)

What Nail shows and where — panes, widgets, message list rows, toolbars,
menus and keyboard shortcuts — comes from one file, `layout.kdl`, in your
Nail configuration folder. How things look is set in
[`theme.kdl`](theme-reference.md).

This page covers what works in the current version (M0).

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
| `account-switcher` | — |
| `folder-tree` | `show-unread` (`#true`/`#false`), `show-account-badges` (`#true`/`#false`) |
| `message-list` | `page-size` (number), `group-by` (`none`, `date`, `sender`), `threads` (`off`, `collapsed`, `expanded`); `row` templates |
| `message-view` | `headers` (e.g. `"from to cc date"`); `row` templates |
| `toolbar` | `toolbar`: the name of a `toolbar` definition (required) |
| `status-bar` | `items` (e.g. `"sync-progress folder-counts"`) |
| `text` | `text`: literal text or `@message.id` |
| `spacer` | — |

The widgets `search-bar`, `thread-view`, `composer`, `calendar`, `agenda`,
`mini-month`, `task-list`, `contact-list` and `contact-card` are accepted
in the file but not available yet. Plug-in widgets are written
`plugin.<plugin-id>.<name>`.

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

Actions in the default layout: `mail.next`, `mail.previous`, `mail.reply`,
`mail.reply-all`, `mail.forward`, `mail.archive`, `mail.delete`,
`mail.toggle-read`, `mail.toggle-flag`, `view.toggle` (argument: the `id` of
a pane, e.g. `"sidebar"`), `sync.now`.

## Keys

```kdl
keys context="list" {
    bind "j" "mail.next"
    bind "g i" "nav.goto-folder \"Inbox\""
    bind "ctrl+shift+m" "view.toggle" "sidebar"
    unbind "k"
}
```

- `context` is where the keys work: `list`, `reader`, … Without `context`
  they work everywhere (`global`).
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
