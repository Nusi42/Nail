# Accessibility: keyboard, screen readers, high contrast, motion

Nail can be used with the keyboard alone, with a screen reader, in high
contrast and without animations.

## Working with the keyboard

- **Tab** and **Shift+Tab** move between the panes and controls. Inside a
  window over the main window (the composer, the settings, a dialog), Tab
  stays in that window: after its last control it goes back to its first.
  **Escape** closes the window, and the focus returns to where you were.
- In the **message list**, `j` / `k` or the arrow keys select the next or
  previous message. In the **folder list**, the arrow keys, Home and End
  move between folders.
- In a **toolbar**, the left and right arrow keys, Home and End move between
  its buttons.
- **Tabs** (for example in the settings) switch with the left and right
  arrow keys, Home and End.
- **Dividers** between panes move with the arrow keys (Shift for bigger
  steps).
- Every other action has a shortcut or can get one: see
  [Reading and organizing mail](mail.md#keyboard-shortcuts) and
  Settings → Customize → Keys (below).

## Context menus

Right-click a message, a message in a conversation, the reader or a folder
for a menu of what you can do with it. From the keyboard, press
**Shift+F10** or the **menu key** on the focused item (the selected message
in the list, the focused folder).

In the menu:

| Key | Does |
| --- | --- |
| Up, Down, Home, End | Move between the entries |
| Enter, Space | Run the entry |
| Right, Left | Open and close a submenu |
| A letter | Jump to the next entry starting with it |
| Escape | Close the menu; the focus goes back |
| Tab | Close the menu |

Entries that cannot run right now (for example "Download full message" for
a message that is already downloaded) are shown dimmed and do nothing. The
menus are defined in `layout.kdl` (see the
[layout reference](layout-reference.md#menus)); Shift+F10 and the menu key
are ordinary shortcuts of the action `menu.context`, so you can change them.

## Changing shortcuts, toolbars and list columns

Settings → Customize has three editors. Each writes your change into
`layout.kdl`, keeping the rest of the file as it is (see
[Customizing](customization.md#editing-toolbars-shortcuts-and-columns)):

- **Toolbars**: reorder, add and remove buttons. Each entry has Up, Down and
  Remove buttons; **Alt+Up** and **Alt+Down** move the focused entry.
- **Keys**: every shortcut by where it works. To add one, choose where and
  what, click the **Keys** field and press the keys (Backspace removes the
  last one; Tab leaves the field).
- **Columns**: show a message list as columns and choose their order.

## Screen readers

Every pane, list, button and field has a name. The message list is a list
box whose selected message is announced; toolbars, tabs, menus and dialogs
use the standard roles, so screen readers describe them as such. Status
messages ("Saved.", "Saving 2 changes…") are announced without moving the
focus; errors are announced at once.

## High contrast

When Windows uses a contrast theme, Nail follows it by itself: the default
theme has high-contrast colours for every part. To pin high contrast (or to
get even stronger contrast, bigger text and thicker focus rings), use the
example theme that comes with Nail, `defaults/themes/high-contrast.kdl`:

1. Copy the file into your configuration folder and name it `theme.kdl`
   (or, if you already have a `theme.kdl`, open Settings → Customize →
   **Apply a theme or layout…** and pick the file to merge it into yours).
2. Nail picks it up at once.

The example extends the default theme, so it stays small. It sets:

```kdl
theme "High contrast" extends="default" mode="high-contrast" {
    tokens {
        color "bg"         "#000000"
        color "fg"         "#ffffff"
        color "focus-ring" "#00ffff"
        border "thin" "2px solid $color.border"
        motion "fast" "0ms"
    }
    part "toolbar.button:focus-visible" {
        outline "3px solid $color.focus-ring"
        outline-offset 2
    }
}
```

See the [theme reference](theme-reference.md) for everything a theme can
change. `?mode=high-contrast` in the browser's address (remote access) turns
high contrast on for that page only.

## Less motion

When Windows is set to show fewer animations (Settings → Accessibility →
Visual effects → Animation effects off), Nail turns its transitions and
animations off too. A theme can also set the `motion` tokens to `0ms`, as
the high-contrast example does.
