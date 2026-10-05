# Reading and organizing mail

## The folder pane

**New message** sits on top. Below it:

- **Favorites** — the same folders of all accounts together: **All
  inboxes**, **Flagged** and **Unread** (searches across every account),
  **Snoozed**, **Drafts**, **Sent** and the **Outbox** (shown only while
  messages wait there; which favorites you see is the `favorites` option of
  the `folder-tree`, see the [layout reference](layout-reference.md#widgets)).
- **One section per account**, in your account order, with the account's
  colour (the same colour marks its messages in the list) and its folders.
  The special folders come first, named in your language — Inbox, Drafts,
  Sent, Archive, Junk, Trash — whatever the server calls them ("INBOX",
  "[Gmail]/Spam"; the server's name is the tooltip); your other folders
  follow. An account that is not online shows its state (for example
  **Offline**, **Disabled**) as a tag beside its name. A folded account
  still shows its inbox's unread count.
- **Smart folders** — your saved searches, only once you saved one.

Click a section's heading (or press Enter / ← / → on it) to fold or unfold
it; Nail remembers what you folded across restarts. The arrow keys move
through the whole pane. Right-click an account's heading for its actions
(check for mail, automatic replies, account settings). Right-click menus
show only what applies to the message or folder: **Not junk**, for
example, appears in the Junk folder and for mail Nail's junk filter
flagged.

## The list header

Above the messages: the search field (the key that focuses it, `/` by
default, is shown in it), the folder's name with its counts (for example
"All inboxes · 102 unread · 4 accounts"), and the filter chips **All**,
**Unread**, **Flagged** and **Attachments**. A chip shows only the matching
messages of the folder; while you search, it narrows your search the same
way (it adds `is:unread`, `is:flagged` or `has:attachment`, see
[Searching](search.md)). When nothing matches, the list says so.

## The action bar and More

The bar above the message shows **Reply** (highlighted), **Reply all** and
**Forward** with their labels, then **Archive**, **Delete**, **Move**,
**Flag** and **Snooze** as icons (the tooltip names the key, for example
"Archive (E)"). Everything else is under **More**, grouped under
**Message** (mark read / unread, junk, labels, categories, print, …) and
**Turn into** (an event, a task, a filter rule), each with its key.
Actions that do not apply to the message (Gmail labels on other mail, for
example) are hidden rather than greyed out. When the pane gets narrow, the
icons move into **More** one by one and come back when there is room.

The message's right-click menu has the same actions in the same groups.

## Conversations

Replies are grouped into conversations: a message and every answer to it,
however many people take part. Your own replies are part of the
conversation even though they sit in the Sent folder.

### Conversations in the list

The message list shows one row per conversation, ordered by its newest
message (your own replies count as activity):

1. **Who** takes part — at most three names, the first ones and whoever
   wrote last, with "me" for you and in bold the people whose mail you have
   not read yet ("Linus, Marie … Ada") — then a small **count** of the
   messages (none for a single message) and the date of the newest one.
2. The **subject**.
3. **Sender: preview** — who wrote the newest message and its first words.

In the Compact density a row has two lines and no preview (Settings →
Appearance, see [Appearance](appearance.md#density)).

A conversation listed in a folder holds that folder's messages of the
conversation plus your own replies from Sent. Drafts never join a
conversation: in Drafts every draft is listed on its own. While you search,
a conversation is made of its messages in every folder except Drafts, Trash
and Junk.

Choosing a row selects the conversation's newest message that you did not
write, and the row's actions (flag, archive, move, delete, the right-click
menu) act on it — never on your own reply kept in Sent.

### The conversation in the reader

Choosing a conversation shows it in the reader as a column of cards, oldest
first, under a header with the subject and the people taking part:

- The newest unread message is open (if you have read everything: the
  newest one). Older messages you have read are collapsed to one line with
  the sender's avatar, name, date and the first words. Click a line (or
  press Enter) to open the card; click its header again to collapse it.
- When more than four read messages lie between the first and the newest,
  they fold into **N older messages**. Click it to show them.
- **Expand all** opens every card; **Collapse all** closes all but the
  newest.
- Your own messages say "me" and are marked as sent by you.
- Quoted text in a reply stays folded (**Show quoted text** unfolds it), so
  each card shows what is new. Cards are not indented: who answered whom is
  in the quote.
- An open card is a full message: its own security badges, the
  remote-content banner, attachments and actions (**Reply**, **Reply all**,
  **Forward**, its right-click menu).

Screen readers hear the conversation as a feed: each card is an article
("3 of 12"), collapsed cards are buttons that say whether they are open.

For Gmail and Exchange accounts the server's own conversations count too:
messages the server keeps together stay together in Nail.

If your own `layout.kdl` was written before the card feed (a `thread-view`
without `mode` above a `message-view`), Nail keeps showing the older strip
and offers once to **Read conversations as cards**. **Use cards** rewrites
the reader in your layout — your per-message widgets, their order and
`mark-read` move into the cards — as one step you can undo under Settings →
Layout & toolbars; **Not now** keeps the strip and does not ask again.

### Keyboard

In the reader (click a card, or press Tab until the conversation has the
focus):

| Key | Action |
| --- | --- |
| `j`, `k` | The next / previous card |
| `Enter`, `o` | Open or collapse the focused card |
| `;` | Expand all |
| `Shift+;` | Collapse all |
| `n`, `p` | The next / previous message, also across a fold |
| `r`, `a`, `f` | Reply, reply all, forward to the focused card's message |

In the list, `j` / `k` and the arrow keys move between conversations. The
keys can be changed in your [`layout.kdl`](layout-reference.md#keys).

### Conversations that open in the list

If you prefer to see the messages of a conversation in the list itself, set
`threads="expandable"` on the `message-list` in your
[`layout.kdl`](layout-reference.md#widgets):

```kdl
message-list id="list" threads="expandable"
```

A conversation row then gets a chevron. Click it (or press `→` on the row;
`←` closes it again) to show the conversation's messages as rows beneath
it, one level deep: sender, first words and date of each message. Choosing
such a message shows just that message in the reader.

With the list focused, `→` on a closed conversation opens it and on an open
one moves to its first message; `←` on a message moves to its conversation
and on an open conversation closes it (the actions `list.expand` /
`list.collapse`). `j` / `k` step through every row, messages included.

To list single messages instead of conversations, set `threads="off"`.

## The message list and the reader

Each row shows the sender's initials in a coloured circle, a dot for unread
mail and, in the Comfortable density, a tag line: the account (when you have
several), attachments, the flag and up to three
labels. The Compact density leaves the tag line out (Settings → Appearance,
see [Appearance](appearance.md#density)).

The reader keeps the header, the attachments and the text in one centred
column at a comfortable reading width; plain-text mail uses the normal UI
font and keeps its line breaks.

HTML mail shows the way its sender designed it: its own style sheet,
colours, backgrounds and pictures sent inside the message. Such a
newsletter keeps its colours on a white page, also in a dark theme, and
gets a wider column; a mail wider than the reader scrolls sideways inside
the message. Pictures from the internet stay blocked until you allow them
(see [Remote content in messages](remote-access.md#remote-content-in-messages)); until then a frame with the picture's
description shows where each one goes.

Attachments are cards with the file type, name and size. **Save** asks
where to save the file in the desktop app (the Save dialog asks before
replacing a file); in a browser the file is downloaded. The card then says
"Saved." or "Downloaded." next to the size. **Preview** appears
when a plug-in can show that type, and the **…** button has the plug-ins'
attachment actions.

## Flag, archive, move, delete

The action bar above the message, the message's right-click menu (or
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
| `Shift+Delete` | Delete for good (asks first: **Delete permanently?**) |
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
time: the message is out of the way until then. **Snoozed** under Favorites
in the folder pane lists them; **Wake up now** brings one back at once. How
snoozing works (also offline and on other devices) is explained in
[Junk mail, snooze and notifications](junk-snooze-notifications.md#snooze).

## Junk mail

**Mark as junk** (`Shift+J`, the toolbar or the message's menu) moves a
message to the Junk folder; **Not junk** moves it back. When a message is
junk, the reader says so above it (and who decided) with a **Not junk**
button. Nail's own junk filter is set up per account in Settings → **Mail & junk**
(the junk filter card): turn it on, choose the threshold and how much training it needs,
see how far it is, or reset it. See
[Junk mail, snooze and notifications](junk-snooze-notifications.md#junk-mail).

## Messages that are not downloaded yet

Per folder you can choose to keep only the headers, or only the last few
days in full: Settings → **Mail & junk** (choose the account and the
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
