# Reading and organizing mail

## The folder pane

**New message** sits on top, with **Check for new mail** (the round arrow)
beside it: it fetches new mail of every account from the servers at once
(also F5 or Ctrl+R). The arrow turns while accounts synchronize; it is off
while the computer has no network, and a failure shows in the status bar.
Below them:

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
flagged. Right-click the pane around its folders for **Check for new mail**,
**New message** and **Add account**.

Where Nail has no menu for a place, a right-click does nothing. On text you
can select — mail, the addresses and subject in the reader, contact
details, keys, error details — and in every text field, a right-click
offers **Cut**, **Copy**, **Paste** and **Select all** (as far as they
apply). Nail's own labels and buttons are not selectable.

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
   not read yet ("Linus, Marie … Ada"; first names when several people are
   named, the full name shows when you point at one) — then a small
   **count** of the messages (none for a single message) and the date of
   the newest one. When the list is too narrow for the names, it leaves out
   names before cutting them short: whoever wrote last and "me" stay.
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
  they fold into **N older messages**, a line between the cards. Click it
  (or press Enter on it) to show them; the focus moves to the first card it
  revealed.
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

In the list, `Enter` or `o` (or a double-click on a message) moves the
focus into the reader, onto the conversation's open card. In a narrow window,
where the list and the reader are tabs, this also switches to the reader's
tab; a single click only selects the message. In the reader (or after
clicking a card):

| Key | Action |
| --- | --- |
| `j`, `k` | The next / previous card |
| `Enter`, `o` | Open or collapse the focused card |
| `;` | Expand all |
| `Shift+;` | Collapse all |
| `n`, `p` | The next / previous message, also across a fold |
| `r`, `a`, `f` | Reply, reply all, forward to the focused card's message |

In the list, `j` / `k` and the arrow keys move between conversations, and
`Enter` / `o` go to the reader. The account of a row is named on its last
line only when the list mixes several accounts (the unified inbox, a search
across accounts). The keys can be changed in your [`layout.kdl`](layout-reference.md#keys).

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

### Links in messages

When you point at a link in a message (or move to it with the keyboard),
a small bar at the bottom edge of the reader shows where it goes, like a
browser's status bar. A very long address is shortened in the middle;
screen readers read it in full.

Clicking a link (also a middle click, or Enter on a focused link) never
opens the page inside the message:

- In the desktop app, web links open in your default browser.
- In a browser (web mode or the remote UI), they open in a new tab.
- An email address (`mailto:`) opens a new message in Nail, addressed to
  it.

When Nail found something wrong with the link (it shows one address but
goes to another, or uses look-alike characters; see
[Warnings in the reader](security.md#warnings-in-the-reader)), the bar
shows the warning, and a click first asks **Open this link?**: **Open
anyway** opens it, **Cancel** (the default) does not.

Attachments are cards with the file type, name and size. **Save** asks
where to save the file in the desktop app (the Save dialog asks before
replacing a file); in a browser the file is downloaded. The card then says
"Saved." or "Downloaded." next to the size. **Preview** appears
when a plug-in can show that type, and the **…** button has the plug-ins'
attachment actions.

Pictures the message shows in its text (a newsletter's logo, a photo
placed between paragraphs) are not listed as cards. **Show all
attachments** lists them too, so you can save them; **Hide images shown
in the message** puts the list back.

## Flag, archive, move, delete

The action bar above the reader, a row's right-click menu (or
**Shift+F10** / the menu key on the selected row) and the keys below act on
the **whole conversation**, like Gmail:

| Action | What it acts on |
| --- | --- |
| Archive, Move, Snooze, Mark as junk | The conversation's messages in the folder you are looking at. Your own replies stay in Sent. |
| Delete | The whole conversation in that account: its messages in every folder (archived ones too), your replies in Sent included — never your drafts, and nothing in Junk. Messages already in Trash stay there. |
| Mark as read / unread, labels and categories | Every message of the conversation. |
| Flag | The newest message you did not write (so the flag shows in the Inbox). |
| Unflag | Every flagged message of the conversation. |
| Delete permanently | Only in Trash or Junk: the conversation's messages there, after a question that says how many messages go. |

**One message only.** Each message card in the reader has its own
**Reply**, **Reply all**, **Forward** and **More** buttons (and its own
right-click menu): **Delete this message**, **Archive this message**,
**Move this message**, **Mark this message unread**, … act on that card's
message alone; that is why the card's menu shows no keys next to them (the
keys act on the whole conversation). In a list that opens conversations into their messages
(`threads="expandable"`), a selected message row acts on that message, and a
list without conversations (`threads="off"`) acts on messages anyway.

**Several conversations.** **Ctrl+click** rows to pick them one by one,
**Shift+click** (or **Shift+↑** / **Shift+↓**) to pick a range; the next
action acts on all of them.

**Undo.** After each action a message at the bottom of the window says what
happened ("Conversation archived", "3 conversations moved to Trash") with
**Undo**. Undo — or `z` up to an hour later — puts every message back where
it was, flags and labels included. Deleting permanently cannot be undone.

**Restore from Trash.** Without Undo (later than an hour, or after the
message has gone), open Trash, select the conversation and move it to the
Inbox (`v`): the mail you received goes to the Inbox, your own replies go
back to Sent.

After you archive, move or delete a conversation, the next one is selected.

### Keyboard shortcuts

| Key | Action |
| --- | --- |
| `u` | Mark as read / unread |
| `Shift+I`, `Shift+U` | Mark as read, mark as unread |
| `s` | Flag / unflag |
| `e` | Archive (to the account's Archive folder) |
| `v` | Move to a folder: type part of its name, then click it or press Enter |
| `Delete`, `#` | Move to Trash (in Trash: delete for good, after a question) |
| `Shift+Delete` | Delete for good (only in Trash or Junk; asks first: **Delete permanently?**) |
| `z` | Undo the last action |
| `r`, `a`, `f` | Reply, reply all, forward (to the newest message, or the card you are on) |
| `b` | Snooze (see below) |
| `Shift+J`, `!` | Mark as junk |
| `l` | Labels (Gmail accounts) |
| `Shift+F10`, menu key | The context menu of the selected row or focused folder |
| `Ctrl+P` | Print the selected message (see [Printing](printing.md)) |
| `Ctrl+U` | Show the selected message's source (see below) |
| `Ctrl+K` | The command palette: every action, searchable (see [Command palette](command-palette.md)) |

Opening a message marks it as read.

## Folders

Right-click a folder in the folder pane for **New subfolder…**, **Rename
folder…**, **Move folder to…** and **Delete folder**; right-click an
account's heading for **New folder…** at the top of the account. The
change is made on the mail server (IMAP and Exchange Online accounts) or
in Nail (POP3 and local accounts):

- Renaming or moving keeps the folder's messages and settings; rules that
  file mail into it follow the new name.
- **Delete folder** moves the folder with its subfolders into the
  account's Trash. Deleting a folder that is in Trash asks first and then
  deletes it with all its messages, on the server too.
- Inbox, Sent, Drafts, Trash and the other special folders cannot be
  renamed, moved or deleted. Gmail accounts have labels instead (**New
  label** in the same menu). Exchange accounts on your own server (EWS)
  cannot change folders from Nail yet.

The window stays open until the server has made the change. When the
server cannot be reached, Nail says so and makes the change as soon as it
can.

## Show source

**Show source** (in **More**, a message's own **…** menu, the right-click
menus, or `Ctrl+U`) opens a window with the complete message as text, to
read, select and copy:

- **Original** — the message exactly as it was received: every header
  (including `Received`, `Authentication-Results` and `DKIM-Signature`),
  the MIME structure and the content as it was sent (often quoted-printable
  or base64).
- **Decoded** — the same, readable: header fields with their encoded words
  decoded, and each text or HTML part decoded to its characters (the HTML as
  the sender wrote it). Attachments and other parts that are not text are
  named in one line instead. An end-to-end encrypted message shows here as
  it is decrypted.

**Wrap lines** breaks long lines at the window's edge; **Copy all** copies
the view you are looking at. Nothing is loaded from the internet to show
the source, and a message larger than 4 MB shows only its beginning. Each
message gets its own window; showing the source again brings it to the
front.

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
