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
- On the **navigation rail**, the up and down arrow keys, Home and End move
  between Mail, Calendar, Contacts, Tasks and the rest; Enter or Space
  shows that view. **Ctrl+1** … **Ctrl+4** (and `g` then `m` for Mail)
  switch views from anywhere, and the focus moves into the view.
- In the **folder pane**, the arrow keys move through Favorites, the account
  sections and their folders; Left and Right fold or unfold a section.
- In a **toolbar**, the left and right arrow keys, Home and End move between
  its buttons. Its **More** button opens the rest of the actions as a menu.
- **Tabs** (for example the folder pane, list and reader of a narrow window)
  switch with the left and right arrow keys, Home and End.
- **Dividers** between panes move with the arrow keys (Shift for bigger
  steps).
- Every other action has a shortcut or can get one: see
  [Reading and organizing mail](mail.md#keyboard-shortcuts) and
  Settings → Keyboard shortcuts (below).

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

Settings → Layout & toolbars has three editors. Each writes your change into
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

High contrast is one of Nail's built-in themes. With **Follow Windows** on
(Settings → Appearance; on by default), Nail switches to it by itself while
Windows uses a contrast theme (Settings → Accessibility → Contrast themes)
and back when you turn the contrast theme off. To use it all the time, click
**High contrast** in the theme gallery of Settings → Appearance.

It has pure black and white with yellow and cyan accents, and every text
and outline reaches at least 7:1 contrast (WCAG AAA). Text is bigger (16 px
body text, which the whole type scale follows), every control has a 2 px
border, focus rings are thick and drawn outside the item, a selected item is
a solid highlight, and there are no animations. Unread messages have a thick
yellow bar on the left, hovered items a dark yellow fill, and avatars and
file-type badges are white with black letters. Nail checks the contrast of
every theme automatically.

Your own changes still apply on top: the text size, corner radius and
accent colour on the Appearance page, and your part rules in `theme.kdl`.
For example, an even thicker focus ring on toolbar buttons:

```kdl
theme "My settings" extends="high-contrast" {
    part "toolbar.button:focus-visible" {
        outline "4px solid $color.focus-ring"
        outline-offset 2
    }
}
```

The theme file itself is `defaults/themes/high-contrast.kdl`; to build a
variant, copy it into the `themes` folder of your configuration folder
under a new name (see [Appearance](appearance.md#your-own-themes)).

See the [theme reference](theme-reference.md) for everything a theme can
change. `?mode=high-contrast` in the browser's address (remote access) turns
high contrast on for that page only.

## Less motion

When Windows is set to show fewer animations (Settings → Accessibility →
Visual effects → Animation effects off), Nail turns its transitions and
animations off too. A theme can also set the `motion` tokens to `0ms`, as
the High contrast theme does.
