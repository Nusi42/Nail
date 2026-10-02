# Reading and organizing mail

## Conversations

Replies are grouped into conversations. When the selected message belongs
to a conversation, the reader shows the other messages above it, oldest
first. Click a message's line to open it in place (quoted text stays
folded; **Show quoted text** unfolds it), or **Open** to show it in the
reader.

For Gmail and Exchange accounts the server's own conversations count too:
messages the server keeps together stay together in Nail.

To list whole conversations instead of single messages, set
`threads="collapsed"` on the `message-list` in your
[`layout.kdl`](layout-reference.md#widgets).

## Flag, archive, move, delete

The toolbar above the message, the message's right-click menu (or
**Shift+F10** / the menu key on the selected message) and these keys act on
the selected message.

### Keyboard shortcuts

| Key | Action |
| --- | --- |
| `u` | Mark as read / unread |
| `s` | Flag / unflag |
| `e` | Archive (to the account's Archive folder) |
| `v` | Move to a folder: type part of its name, then click it or press Enter |
| `Delete` | Move to Trash (in Trash: delete for good) |
| `Shift+Delete` | Delete for good |
| `r`, `a`, `f` | Reply, reply all, forward |
| `b` | Snooze (see below) |
| `Shift+J` | Mark as junk |
| `Shift+F10`, menu key | The context menu of the selected message or focused folder |
| `Ctrl+P` | Print the selected message (see [Printing](printing.md)) |
| `Ctrl+K` | The command palette: every action, searchable (see [Command palette](command-palette.md)) |

Opening a message marks it as read. The next message is selected after you
archive, move or delete one.

## Snooze

Press `b`, click **Snooze…** or choose it in the message's menu, then pick
**Later today**, **Tomorrow**, **This weekend**, **Next week** or a date and
time: the message is out of the way until then. **Snoozed messages** in the
main toolbar lists them; **Wake up now** brings one back at once. How
snoozing works (also offline and on other devices) is explained in
[Junk mail, snooze and notifications](junk-snooze-notifications.md#snooze).

## Junk mail

**Mark as junk** (`Shift+J`, the toolbar or the message's menu) moves a
message to the Junk folder; **Not junk** moves it back. When a message is
junk, the reader says so above it (and who decided) with a **Not junk**
button. Nail's own junk filter is set up per account in Settings → **Junk
mail**: turn it on, choose the threshold and how much training it needs,
see how far it is, or reset it. See
[Junk mail, snooze and notifications](junk-snooze-notifications.md#junk-mail).

## Messages that are not downloaded yet

Per folder you can choose to keep only the headers, or only the last few
days in full: Settings → **Synchronization** (choose the account and the
folder), or **Synchronization…** in a folder's right-click menu. "As the
account" removes the folder's own setting. Such messages are marked in the
list; opening one downloads it. If that fails (for example offline), the
reader says so and offers **Download full message** to try again. POP3
accounts always keep full messages; Exchange accounts follow these
settings too.

## Working offline

Every change takes effect in Nail at once and is then sent to the server in
the background. The status bar shows "Saving 2 changes…" while that is
happening. When you are offline, changes wait and are sent when the
connection is back. If the server refuses a change, the status bar says so
and Nail shows the server's state again.
