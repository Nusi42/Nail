# Microsoft Exchange and Microsoft 365

Nail connects to Microsoft Exchange directly: Microsoft 365 / Exchange
Online through Microsoft Graph, and Exchange Server in your organization
through Exchange Web Services (EWS). Mail, calendars, contacts and tasks
come over the same connection. The settings of the `exchange` node are in
the [accounts reference](accounts-reference.md#microsoft-exchange-exchange).

## Adding an Exchange account

1. Choose **Add account** and enter your name and address.
2. Under **Kind of account**, leave **Find the settings automatically**,
   or say what you have:
   - **Microsoft 365 or Exchange Online** — Nail connects through
     Microsoft Graph and you sign in with Microsoft in your browser.
   - **Exchange Server in my organization** — enter the server (for
     example `mail.example.com`); ask your administrator if you do not
     know it.
3. When Nail finds the settings with Microsoft Autodiscover, it shows what
   it found: the connection (Graph or EWS), the server and the EWS address.
   You can switch between Graph and EWS here.

### When the settings point to another server

Autodiscover can send Nail to a server outside your address's domain. That
is normal for some hosted Exchange services, but it is also what an
attacker would do to collect passwords. Nail then shows **The settings for
… point to …** and does not continue until you tick **I trust …**. Do that
only if your organization really uses that server.

Nail also warns when an account uses EWS with Exchange Online: Microsoft is
retiring EWS there, so switch such an account to Microsoft Graph.

### Signing in to an Exchange server

For Exchange Server in your organization you can sign in:

- **As the user signed in to Windows** — like Outlook. Nail stores no
  password. **Sign-in protocol** can force Kerberos or NTLM; leave
  **Automatic** unless your administrator says otherwise.
- **With another Windows account** — enter it as `DOMAIN\user` and its
  password; the password is kept in the system's credential store.
- **With a password** (Basic authentication over an encrypted connection).
- **With Microsoft (modern authentication)** for hybrid setups.

Microsoft 365 accounts always sign in with Microsoft in the browser.

## Shared mailboxes and delegate access

Mailboxes your organization gives you (shared mailboxes such as a team's
support address) appear by themselves under the account, each as its own
folder tree. To open another one, use **Add shared mailbox…** in the
**Other mailboxes** list of the folder pane, enter its address and choose
what to show: mail, calendar, contacts, tasks. Choose **Another person's
mailbox you have delegate access to** for a manager's calendar, for
example.

The list shows each mailbox's state: **Not synced yet**, **No access to
this mailbox** (ask the owner or your administrator for permission),
**No such mailbox** or **Syncing failed**. **Remove** takes a mailbox out
of Nail; nothing changes on the server. A mailbox your organization added
is hidden instead, and comes back when you add it again.

## Categories

Exchange categories are Nail tags with the category's colour. The
**Categories** list in the folder pane shows them with their unread
counts; choose one to see its messages in every folder. **New category**,
**Edit** and **Delete** change the category list on the server:

- Outlook's 25 colours (or none) are available.
- Renaming a category renames it on every message that has it.
- Deleting a category removes it from the list; messages keep it, without
  a colour (as in Outlook).
- Category names cannot contain commas or semicolons.

## Focused Inbox

If your server sorts mail into Focused and Other, tabs above the message
list show **All**, **Focused** and **Other** for the inbox. You can also
search with `focus:focused` or `focus:other`. **Move to Other** /
**Move to Focused** moves a message, and **Always move messages from …**
teaches the server to sort that sender's future mail the same way. The
**Focused Inbox** switch hides the tabs (it changes nothing on the
server).

## Automatic replies (out of office)

**Automatic replies…** opens the editor. Exchange answers for you even
while Nail is closed:

- **Send automatic replies**, or **only during this time** with a start
  and end (in your time zone).
- **Reply to people in my organization** and, separately, **to people
  outside my organization** — to nobody, only your contacts, or everyone.
- Replies are plain text. Formatting set in Outlook is replaced when you
  save here.

While replies are on, a banner says so, with **Turn off** and **Edit…**.
"Saved in Nail; not on the server yet" means the change is still on its
way to the server.

## Rules on the server

For Exchange accounts, Nail places your rules as Exchange inbox rules where
Exchange can run them, so they work while Nail is closed; the others run in
Nail. The rule list shows for each rule where it runs on each Exchange
account, and why it stays in Nail (for example "uses a search query").
**Settings → Exchange inbox rules** shows every inbox rule on the server:
rules created in Outlook can be switched on or off or deleted there, and
**Sync now** writes your rules again. **Run rules on the server where
Exchange can** switches the placing off for an account.

## Booking a room

In the event editor of an event in an Exchange calendar, **Find a room…**
lists your organization's rooms for the event's time: all rooms or one
room list, with at least so many seats, or only free ones. Each room shows
its seats, building and floor, accessibility, equipment and whether it is
free (with its busy times otherwise). **Add** invites the room (and sets
the location if it is empty); the room's answer appears under **Booked
rooms**. If Nail says that rooms need the "Place.Read.All" permission, ask
your administrator to grant it.

## Microsoft To Do

Your To Do lists appear in the task widget under **Microsoft To Do —
<account>**, with **Tasks** first and **Flagged email** last; task lists of
shared mailboxes follow, then your other lists.

## Layout options

| Widget | Options |
| --- | --- |
| `focused-tabs` | `show-all` (default `#true`): the **All** tab; `show-switch` (default `#true`): the **Focused Inbox** switch |
| `automatic-replies-banner` | `only-active-account` (default `#false`): only for the selected account |
| `exchange-mailboxes` | — |
| `exchange-categories` | `show-counts` (default `#true`), `show-empty` (default `#true`): categories no message has |
| `exchange-rules` | — |

Actions: `exchange.add-mailbox`, `exchange.new-category`,
`exchange.automatic-replies`, `exchange.focus "focused"` / `"other"` (no
argument: the whole inbox), `exchange.classify "focused"` / `"other"` (no
argument: the other tab), `exchange.toggle-focused`.
