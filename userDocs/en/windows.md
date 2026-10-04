# Windows: moving, resizing, snapping and pop-out windows

Everything that opens over Nail's main window — the composer, Settings
windows, filter rules, the plug-in manager, the event editor, wizards,
pickers and questions — is a real window. You can keep working in the main
window next to it, and you can have several open at once.

## Moving and resizing

- **Move** a window by dragging its title bar. A window always keeps at
  least a strip of itself on the page, and its title bar never goes above
  the top edge.
- **Resize** it from any of its four edges or four corners. Every window has
  a smallest size it will not go below.
- **Maximize** it with the button in its title bar or by **double-clicking
  the title bar**. Do the same again to restore it to where it was.
- **Snap** it to the left or right half of the page: drag it by the title
  bar until the pointer touches the left or right edge. An outline shows the
  half it will fill; let go to snap. Drag a snapped or maximized window away
  and it comes loose at its previous size, under the pointer.

Windows that only ask a question have a fixed size and cannot be maximized
or snapped.

When the main window gets smaller, the windows inside it shrink and move to
stay visible. When it grows again, they go back to where you put them.

## Using the keyboard

| Keys | What they do |
| --- | --- |
| `Tab` / `Shift+Tab` | Move between the controls of the window. Focus stays inside the window. |
| Arrow keys on a focused title bar | Move the window by 16 pixels. |
| `Shift` + arrow keys on a focused title bar | Make the window larger or smaller by 16 pixels. |
| `Alt+F7` | Move mode, anywhere in the window: arrow keys move it, `Enter` keeps the new place, `Esc` puts it back. |
| `Alt+F8` | Size mode: arrow keys resize, `Enter` keeps, `Esc` puts it back. |
| `Ctrl+F6` | From the main window, go to the window in front. From a window, bring the next one forward. With one window open, switch between it and the main window. |
| `Ctrl+Shift+F6` | Back to the main window, to where you were. |
| `Esc` | Close the window you are in. The composer asks first if it has changes that are not saved. |

Screen readers announce each window by its title. The title bar is
announced as "Title bar of …", together with how to move and resize it, and
the move and size modes, maximizing, restoring and snapping are announced as
they happen.

The command palette (`Ctrl+K`) also has **Next window**, **Back to the main
window**, **Maximize or restore window** and **Reset window positions**.

## Nail remembers where you put windows

Nail remembers each kind of window's size and place — for example where you
like the composer or the filter rules window — and opens it there next time,
also after Nail restarts. When a second window of the same kind opens (a
second message, say), it opens a little below and to the right of the first,
so both stay visible.

The positions are kept in `window-state.json` in your configuration folder.
They are not part of your layout or theme, and backups leave them out.
**Reset window positions** (command palette) forgets them all and puts open
windows back in their default place. Browsers connected through remote
access share the desktop's positions, fitted to their own page.

## Which windows block the rest of Nail

Almost no window does. The composer, Settings, filter rules, the plug-in
manager, the calendar and contact editors and the other tool windows let you
keep working everywhere else.

Only short questions that must be answered first are **modal**, and they
block only the place they came from:

- Questions about one window — closing a message with unsaved changes,
  discarding a draft, a plug-in stopping a message ("Send anyway?"), which
  occurrences of a repeating event to change — block **only that window**.
  It is dimmed until you answer. The main window and other windows keep
  working.
- Questions from the main window — **Delete permanently?**, removing a
  calendar or a label — dim the main window until you answer. Other open
  windows, such as a composer, keep working.

A question about one window opens over that window and stays in front of
it; click another window and that window comes in front of both. A question
from the main window stays in front of every window. `Esc` or its × button
answers it the safe way (keep editing, cancel).

A new window of a kind that is already open (a second message, say) opens a
little lower and to the right, never exactly on top of an open one.

## Pop-out windows (desktop app)

In the desktop app, a window can leave Nail's main window and become a
window of its own on your desktop — for example to write a message on a
second monitor while you read mail on the first.

- Click **Open in new window** in the title bar (the icon with an arrow
  pointing out of a box), or use **Open window in new window** in the
  command palette. It is offered for messages, Settings, filter rules, the
  outbox and snoozed messages. Editors that work on what is selected in the
  main window (events, contacts, tasks, labels, categories), questions and
  the folder picker stay in Nail's main window.
- A message's questions (Discard?, unsaved changes, a plug-in stopping it)
  show in its own window.
- Closing a pop-out window with its × button or `Alt+F4` is like closing
  it inside Nail: a message with unsaved changes asks whether to save them
  first.
- The new window has its own taskbar entry. A message's window is titled by
  its subject.
- Everything you do there reaches Nail at once: a message sent from a
  pop-out window is in the outbox, a setting changed there applies
  everywhere.
- **Return to main window** at the top of the pop-out window puts it back
  into Nail's main window. A message keeps what you typed.
- Nail remembers where each kind of window was on your desktop, on which
  monitor and whether it was maximized. When that place is no longer
  available (a monitor was unplugged), the window opens over Nail's main
  window instead. A kind of window you last used popped out opens popped
  out again.
- Closing Nail's main window (also to the tray) closes its pop-out windows.
  Messages are not lost: they were saved as drafts.

In a browser (`nail serve`, remote access) every window stays inside the
page.

## For theme authors

The window manager's look is made of parts you can style in `theme.kdl`;
see [the theme reference](theme-reference.md#parts). A window's size and
place are not theme properties: set a window's default and smallest size in
`layout.kdl` with `size=` and `min-size=` (see
[the layout reference](layout-reference.md)).
