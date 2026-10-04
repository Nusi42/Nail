# Customizing the layout and the toolbars

Nail lets you rearrange its windows and toolbars inside the app. Every
change is written to `layout.kdl` in your configuration folder, the file
the [layout reference](layout-reference.md) describes. Your comments, blank
lines and everything you did not touch stay as they were, and every change
can be undone.

## Customize layout

**Customize layout** turns the view you are looking at (Mail, Calendar,
Contacts or Tasks) into an editor. Start it in one of these ways:

- right-click the navigation rail and choose **Customize layout** (the
  View menu);
- press `Ctrl+K` and run **Customize layout** from the
  [command palette](command-palette.md);
- open **Settings → Layout & toolbars**, choose the view under **Window
  layout** and click **Customize layout**.

A banner at the top says which view you are customizing and holds **Undo**,
**Redo**, **Reset to default** and **Done**. Every pane gets a header with
its name, a drag handle, an eye to hide or show it and a button that removes
it. The panes keep showing their content, but you cannot use it while the
editor is open. **Done** (or `Esc` when nothing is picked up) ends the mode.

### Moving and swapping panes

Drag a pane by its handle onto another pane. While you drag, every other
pane shows its drop targets:

| Target | What happens |
| --- | --- |
| **Left**, **Right**, **Above**, **Below** | The pane is docked on that side of the target pane. |
| **Swap here** | The two panes change places (and sizes). |
| **Add here** | A widget from the palette goes into that pane (below its content). |

Nail writes the change as soon as you drop. In the mail view, for example,
**Swap here** on the reader puts the folders on the right and the reader on
the left.

### With the keyboard

Every drag has a keyboard way:

1. Tab to a pane's handle (**Move Folders**, for example) and press `Enter`
   or `Space`. The pane is picked up and the drop targets appear.
2. `Tab` to a target — each says what it does, such as "Swap Folders with
   Message" or "Put Message below Messages" — and press `Enter`.
3. `Esc` puts the pane back without changing anything.

Screen readers announce each step and the result.

### Hiding and removing panes

The eye (**Hide Folders**) hides a pane: Nail writes `hidden=#true` on it
and the other panes take its room. In the editor a hidden pane keeps its
header, so you can show it again with the same eye. **Remove** takes a pane
out of the layout; **Undo** or **Reset to default** brings it back.

### Presets

The palette on the right starts with presets for the mail view:

| Preset | Arrangement |
| --- | --- |
| **Three panes** | Folders, messages and the reader side by side (the default). |
| **Reader below** | Folders on the left; the message list above the reader. |
| **Wide reader** | The message list and a wide reader; the folders are hidden. |
| **Focus** | Only the reader; folders and list are hidden. |

The preset that matches the current arrangement is shown pressed. A preset
is unavailable when the view no longer has a pane it needs (because you
removed it); **Reset to default** brings the panes back. Presets are also on
the **Window layout** card in Settings, without starting the editor.

Presets are defined in the `presets` section of `layout.kdl` (see the
[layout reference](layout-reference.md#layout-presets)); you can add your
own.

### Adding widgets

Under **Add to layout** the palette offers widgets to place in any pane:
**Mini month**, **Today's agenda**, **Task list**, **Smart folders**, and
the panels of the plug-ins you have enabled. Drag one onto a pane's **Add
here** target, or press it and choose the target with the keyboard. Remove
it again with its pane header's **Remove**.

### Undo, Reset to default and changes outside Nail

- **Undo** and **Redo** step through your changes; their tooltip says what
  they undo ("Undo: Swap panes"). The history also holds changes made from
  other places that write `layout.kdl` — dividers you dragged, the toolbar
  customizer, the merge dialog — and lasts until Nail quits.
- **Reset to default** puts the whole view back to the default layout (all
  of its breakpoints). Your other windows stay as they are.
- If `layout.kdl` was changed outside Nail since the editor last read it (in
  a text editor, say), Nail does not write over it: the editor says that the
  file was changed outside Nail. **Undo** then offers **Undo anyway**, which
  first copies the current file to `backups/` and then restores the version
  from before your change.

## Customize a toolbar

Right-click any toolbar — the reader's action bar, the calendar's toolbar
or one of your own — and choose **Customize toolbar**. With the keyboard,
press `Shift+F10` or the menu key on one of its buttons. The command palette
and **Settings → Layout & toolbars → Toolbars** open the same window; there
you pick the toolbar first.

The window has three lists:

- **Toolbar**: the buttons on the bar, in order, with separators and
  flexible spaces.
- **More menu**: the entries of the bar's **More** menu, with its headings.
- **Available actions**: separator, flexible space, then every action Nail
  has, including plug-in commands (marked **Plug-in**). Actions already on
  the toolbar are marked **On toolbar**. Type in **Find an action** to search;
  the list shows the first 30 matches.

Drag an entry between the lists: dropping on an entry puts it before or
after that entry, dropping on a list's empty space adds it at the end, and
dropping a toolbar or More entry on **Available actions** removes it. A
button moved into More keeps its label and icon.

With the keyboard:

| Keys | In Toolbar / More menu | In Available actions |
| --- | --- | --- |
| Arrow keys, `Home`, `End` | choose an entry | choose an action |
| `Ctrl` + arrow keys | move the entry earlier / later | — |
| `M` | move it between the toolbar and More | — |
| `Delete` | remove it | — |
| `Enter` / `Shift+Enter` | — | add it to the toolbar / to More |

The buttons under the lists (**Move earlier**, **Move later**, **Move to
More**, **Remove**, **Add to toolbar**, **Add to More**) do the same for the
chosen entry. While the window is open, the toolbar it edits is outlined.

**Undo** and **Redo** use the same history as Customize layout. **Reset to
default** gives the toolbar its default buttons again; **Done** closes the
window. A toolbar changed on disk is handled as described above (**Undo
anyway**).

## What Nail writes

The first change copies the window or toolbar you changed from the default
layout into your `layout.kdl` (creating the file if needed); everything else
keeps coming from the default. For example, after swapping the folders and
the reader and hiding the message list:

```kdl
layout "My layout" extends="default" {
    breakpoint min-width=900 {
        window "mail" {
            split "horizontal" id="panes" sizes="1.4fr 1fr 260px" {
                stack id="reader" label="@layout.pane.message" { /* … */ }
                stack id="list-pane" label="@layout.pane.messages" hidden=#true { /* … */ }
                stack id="sidebar" label="@layout.pane.folders" { /* … */ }
            }
        }
    }
}
```

A pane takes its comments with it when it moves. See also
[Customizing](customization.md) for the inspector, the shortcut and column
editors and the merge dialog, and [Windows](windows.md) for moving and
resizing the windows themselves.
