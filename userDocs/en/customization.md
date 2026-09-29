# Customizing Nail: inspector, write-back, merging and errors

Nail's look comes from `theme.kdl` and its arrangement from `layout.kdl`
(see the [theme reference](theme-reference.md) and the
[layout reference](layout-reference.md)). This page shows the tools that
help you change them:

- the **inspector** tells you what any part of the window is called and
  which rules style it;
- changes you make **in the window** (dragging a divider) are written into
  `layout.kdl` for you;
- the **merge dialog** applies a theme or layout someone shared without
  overwriting your own;
- the **error list** shows every mistake in your files at once;
- **smart folders** keep searches you use often one click away.

## The inspector

Turn the inspector on with the action `inspector.toggle` (bind it to a key,
see below). While it is on:

- point at anything in the window: it is outlined and the inspector panel
  shows it;
- click to keep the panel on that element (the click does not press
  buttons while the inspector is on); "Pick another element" lets you point
  again;
- press **Esc** (or run the action again) to stop.

For the element the panel shows:

| Section | What it tells you |
| --- | --- |
| Part path | The part names from the window down to the element, e.g. `app › split › message-list › message-list.row › message-list.row.subject`. Use the last one (or an ancestor) in a `part "…"` rule of your theme. |
| States | States such as `:selected` or `:unread`, and data conditions such as `[tag="work"]` — what you can write after the part name in a rule. |
| Theme rules | Every rule of the active theme that styles the element, with its declarations and the file and line it comes from. Rules that only apply while hovering or focused say so. |
| Layout node | The widget or container that placed the element: its kind and id, the window, the breakpoint and its position in the tree, and — for widgets — the line in `layout.kdl`. |

**Open in editor** opens the file at that line in your editor. Nail uses
the editor named in the environment variable `NAIL_EDITOR`, `VISUAL` or
`EDITOR` (in that order), for example `code`, `notepad++`, `nvim` or
`subl`; they open at the right line. Any other program gets the file name;
a value containing `{file}` is used as a command template, with `{line}`
and `{column}` filled in too:

```
NAIL_EDITOR=myeditor --goto {line}:{column} {file}
```

Without a setting, Windows opens the file in Notepad. Positions inside the
shipped defaults (`defaults/theme.kdl`, `defaults/layout.kdl`) are shown but
cannot be opened: copy what you want to change into your own file instead.
In the browser version, opening files is not available.

A key for the inspector, and for the other tools on this page:

```kdl
keys {
    bind "ctrl+shift+i" "inspector.toggle"
    bind "ctrl+shift+e" "customize.errors"
}
```

## Changes in the window are written to layout.kdl

When you drag a divider between panes (or move it with the arrow keys),
Nail writes the new sizes into `layout.kdl` once you let go. The file stays
yours: comments, blank lines, indentation and the order of everything else
are kept exactly as they were, and only the `sizes="…"` value changes.

- If you have no `layout.kdl` yet, Nail creates a short one that extends the
  default layout.
- If the window you changed only exists in the default layout, Nail first
  copies that window (all of its breakpoints) into your file, then changes
  it. Everything else keeps coming from the default.
- If your `layout.kdl` has an error, nothing is written until you fix it.

The same write-back handles toolbar contents and order, key bindings
(removing a default binding writes `unbind "<key>"`) and the column order of
a list (`columns="…"` on `message-list`).

## Applying someone else's theme or layout: the merge dialog

Open the dialog with the action `customize.merge` (for example from a
toolbar: `item "customize.merge"`), then:

1. Choose whether it is a theme or a layout, and load the file or paste its
   text.
2. Choose **Compare with mine**. Nail lists the differences by section —
   name, tokens, colour mode, part styles, icons, message display and
   sounds, raw CSS for a theme; windows, actions, key bindings, toolbars
   and menus for a layout. Each item shows your version and theirs.
3. For each item choose **Keep mine** or **Take theirs**, or use the buttons
   of a section or **Take all theirs** / **Keep all mine**. Items you do not
   touch stay yours.
4. With **Live preview** on, the window shows the result of your choices
   while you choose. Nothing is saved yet.
5. **Apply** writes the result into your file. Your previous file is kept as
   a backup; **Revert** right after applying, or later under
   **Earlier versions**, brings it back.

Your own file stays as it is except for the items you took: comments and
formatting around them are kept, and taking *everything* writes their file
as it is.

Conflicts are marked on the item before you take it:

| Conflict | Meaning |
| --- | --- |
| Key already in use | Their binding uses a key you bound to something else (or a key sequence that starts with, or is the start of, one of yours). |
| Unknown action | An action that does not exist here, usually from a plug-in you do not have. |
| Widget not installed | A plug-in widget you do not have. |
| Drops plug-in items | Taking it removes plug-in buttons or widgets you placed. |
| Cannot be taken as it is | For example a style that uses a colour token only their theme defines (take that token too), or an item the defaults define that a file extending the defaults cannot remove. |

If the file you loaded has mistakes, the dialog lists all of them with line
and column instead of comparing.

## All configuration errors at once

When `theme.kdl`, `layout.kdl`, `accounts.kdl` or `rules.kdl` has mistakes,
Nail keeps using the last version that worked and reports **every** error of
each file with its line and column — you can fix them all in one go. Each
entry has **Open in editor**.

The list is the `config-errors` widget; the action `customize.errors` opens
it as a dialog. To keep it in a pane that only appears when something is
wrong:

```kdl
window "main" {
    stack sizes="auto 1fr" {
        config-errors hide-when-valid=#true
        message-list
    }
}
```

| Option | Values | Default | Meaning |
| --- | --- | --- | --- |
| `hide-when-valid` | `#true`, `#false` | `#false` | Show nothing while all files are valid. |

## Smart folders (saved searches)

A smart folder is a saved search. Search as usual (see
[search](search.md)), then choose **Save current search** in the smart
folders list and give it a name. Choosing a smart folder runs its search
again — always up to date, over all accounts or over the account or folder
the search was limited to. The × next to a smart folder removes it (the
messages stay where they are).

Place the list anywhere in your layout with the `smart-folders` widget, for
example under the folder tree:

```kdl
window "main" {
    split sizes="240px 1fr" {
        stack sizes="auto 1fr" {
            folder-tree
            smart-folders
        }
        message-list
    }
}
```

| Option | Values | Default | Meaning |
| --- | --- | --- | --- |
| `editable` | `#true`, `#false` | `#true` | Show "Save current search" and the remove buttons. |

## Actions

| Action | What it does |
| --- | --- |
| `inspector.toggle` | Turns the inspector on or off. |
| `customize.merge` | Opens the merge dialog. |
| `customize.errors` | Opens the list of configuration errors. |
