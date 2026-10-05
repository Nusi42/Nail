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

### What Autodiscover sends where

Autodiscover first asks your domain's own servers and then — only when
your domain's mail is handled by Microsoft (its MX records point to
Microsoft) — Microsoft's global Autodiscover service, which learns the
address you are setting up. Nail never sends a password while
looking up the settings. It signs in as the user signed in to Windows only
when a server of your address's domain asks for it and you choose **Look
again with my Windows sign-in** — and only to the server that prompt
named: if another server asks too, Nail asks you again. If a server of your domain points to a
server outside it, or the settings were found through the unencrypted
"HTTP redirect" method or a DNS record, Nail asks first (**Trust … and
look again**) before anything goes there.

If nothing is found, choose **Microsoft 365 or Exchange Online** or
**Exchange Server in my organization** on the first page and enter the
server yourself.

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

**Moving messages** between your mailbox and a shared or delegate mailbox
works like any move (drag, **Move to…**): Exchange moves the message on
the server. Moving a message into another account works too between
Exchange and Gmail accounts (and from IMAP or POP3 accounts into them):
Nail copies it with its read and flagged state into the other account and
then removes it from the first. Categories and labels are not carried
over.

**Sending from a shared mailbox**: add an identity with the mailbox's
address to the account and choose how it sends (`send-as` in
`accounts.kdl`, see [Accounts reference](accounts-reference.md)): **as**
the mailbox (recipients see only the mailbox; needs the "Send As"
permission) or **on behalf of** it (recipients see "you on behalf of the
mailbox"; needs "Send on Behalf"). The sent copy is filed in the shared
mailbox's Sent Items.

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

**Categories…** in the message toolbar (and in its right-click menu) sets
the categories of the selected conversation (every message of it; **Undo**
reverts them): they are written to the server as the messages' categories.

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

**Settings → Server rules & vacation** works for Exchange accounts too: it sets the
automatic replies, with the same text for people inside and outside your
organization. Use the editor above for different texts.

## Rules on the server

For Exchange accounts, Nail places your rules as Exchange inbox rules where
Exchange can run them, so they work while Nail is closed; the others run in
Nail. The rule list shows for each rule where it runs on each Exchange
account, and why it stays in Nail (for example "uses a search query").
**Settings → Server rules & vacation** shows every inbox rule on the server:
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

## Meetings

Invitations in an Exchange calendar are answered through Exchange: when
you accept, decline or answer tentatively, Exchange sends your answer to
the organizer — Nail sends no invitation mail of its own. Meetings you
organize in an Exchange calendar are sent to the attendees by Exchange
when you save them, and cancelled when you delete them. Free/busy of
people and rooms of your organization comes from Exchange.

Changing your own copy of a meeting you were invited to (its reminder,
categories, notes or how it shows your time) updates only your calendar:
the organizer hears from you only when you change your answer. A comment
you write with your answer goes to the organizer with it.

## Microsoft To Do

Your To Do lists appear in the task widget under **Microsoft To Do —
`<account>`**, with **Tasks** first and **Flagged email** last; task lists of
shared mailboxes follow, then your other lists. A task's checklist items
are its subtasks. A subtask you add in Nail shows briefly as gone after it
is saved and comes back with the next sync (Exchange gives it its id).

## Good to know (Microsoft 365)

- Messages Nail files into a folder (other than drafts) appear as drafts in
  Outlook on the web: Microsoft Graph creates every uploaded message as a
  draft. Sent mail is filed by Exchange itself.
- To Do lists are synced for your own mailbox only (shared mailboxes show
  their calendars and contacts).
- Calendars are synced from one year back to two years ahead.
- The room finder needs the `Place.Read.All` permission, which your
  administrator grants once for the organization.
- **Large messages**: Exchange Online takes messages up to its own size
  limit (usually 25–150 MB, set by your organization). Nail uploads
  messages over 4 MB in pieces and shows the upload progress on the
  message in the outbox and in the status bar. Signed or encrypted messages over 4 MB cannot be sent through
  Microsoft Graph; use a smaller attachment or a link.
- Sending a large message (over 4 MB) as a shared mailbox needs full
  access to that mailbox (the upload goes through its drafts); "Send As"
  alone is enough for smaller messages.

## Good to know (Exchange Server)

- Exchange Server in your organization has no Focused Inbox: the tabs say
  so, and **Always move messages from …** is not available.
- If Outlook keeps rules that only Outlook can run ("client-only" rules),
  Exchange refuses changes to the rule list until they are removed in
  Outlook; Nail then keeps running your rules itself.
- Answering an invitation without sending your answer only updates your
  calendar.
- Rooms listed by Exchange Server show no seats or building.
- Events you create or change are saved with your time zone, so a weekly
  meeting stays at the same time after the change to or from daylight
  saving time. A recurring event in a time zone Exchange Server does not
  know is not saved to the server (Nail reports the change as failed); a single event
  in such a zone is saved at the same moment in UTC.
- New mail arrives at once: Exchange Server tells Nail about changes
  (streaming notifications); Microsoft 365 is asked every few seconds to
  minutes, more often while you work.

## Good to know (all Exchange accounts)

- When Outlook, your phone or a rule on the server moves a message, Nail
  moves its copy too: the message keeps its tags, snooze and other things
  you set in Nail, and it does not count as new mail.
- The first sync of a large mailbox can take several rounds (Exchange
  limits how fast it may be read). Mail found during that first sync is
  never treated as new: no notifications and no rules run on it.
- **Folder sync policies** (**Synchronization…** in a folder's menu:
  everything, headers only, or the last N days) apply to Exchange folders
  too: messages outside the policy keep their headers, and their body is
  downloaded when you open them.
- Messages of one Exchange conversation are shown as one conversation in
  Nail, also when they have no reply headers linking them.

## Moving from EWS to Microsoft Graph

Microsoft is retiring Exchange Web Services in Exchange Online. An account
set up with `engine="ews"` against Exchange Online shows a warning; switch
it to **Microsoft Graph (Exchange Online)** in the account settings (or
`engine="graph"` in `accounts.kdl`). Microsoft issues sign-ins per
service, so you sign in with Microsoft once more after the switch. Shared
mailboxes on Exchange Online are added by their address (**Add shared
mailbox…**): only Exchange Server in your organization lists them
automatically.

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
