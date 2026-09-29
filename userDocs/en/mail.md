# Reading and organizing mail

## Conversations

Replies are grouped into conversations. When the selected message belongs
to a conversation, the reader shows the other messages above it, oldest
first. Click a message's line to open it in place (quoted text stays
folded; **Show quoted text** unfolds it), or **Open** to show it in the
reader.

To list whole conversations instead of single messages, set
`threads="collapsed"` on the `message-list` in your
[`layout.kdl`](layout-reference.md#widgets).

## Flag, archive, move, delete

The toolbar above the message and these keys act on the selected message:

| Key | Action |
| --- | --- |
| `u` | Mark as read / unread |
| `s` | Flag / unflag |
| `e` | Archive (to the account's Archive folder) |
| `v` | Move to a folder: type part of its name, then click it or press Enter |
| `Delete` | Move to Trash (in Trash: delete for good) |
| `Shift+Delete` | Delete for good |
| `r`, `a`, `f` | Reply, reply all, forward |

Opening a message marks it as read. The next message is selected after you
archive, move or delete one.

## Working offline

Every change takes effect in Nail at once and is then sent to the server in
the background. The status bar shows "Saving 2 changes…" while that is
happening. When you are offline, changes wait and are sent when the
connection is back. If the server refuses a change, the status bar says so
and Nail shows the server's state again.
