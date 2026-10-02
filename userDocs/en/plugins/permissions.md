# Plug-in permissions

A plug-in lists the permissions it needs in its manifest. The install
dialog shows them, and the plug-in can use only what you granted. Every
call a plug-in makes is checked; a call without its permission fails with
the error code `forbidden` and is written to the plug-in's log. A hook,
preview or job whose permission you did not grant (or revoked) is simply
not called.

## Reading

| Permission | What it allows | API |
| --- | --- | --- |
| `mail.read` | Reading your accounts, folders and message headers: subject, sender, recipients, date, flags, tags. Not the message text: searches look at these header fields only. Can be limited to some accounts. | `nail.mail.get`, `nail.mail.search`, `nail.mail.thread` |
| `mail.read.body` | Reading the text of messages too (plain text, cleaned-up HTML, attachment names and sizes, never attachment content), and searching it. Needs `mail.read`. Never the content of end-to-end encrypted mail: while you let Nail index decrypted mail for searching, plug-in searches leave encrypted mail out. Can be limited to some accounts. | `nail.mail.getBody`, `nail.mail.search`; the body for arrival and open hooks |
| `calendar.read` | Reading the events of the calendars you show: title, time, place, and whether they repeat (not attendees or descriptions). | `nail.calendar.events` |

## Changing your data

These are marked *Can change your data* in the install dialog. Every
change goes through Nail's operation queue — the same undo history and
log as your own changes — and follows the plug-in's
[confirmations](writing-data.md#confirmations): deleting always asks you
first; sending, invitations and filter rules ask unless you allow them;
the rest runs without asking unless you make it ask.

| Permission | What it allows | API |
| --- | --- | --- |
| `mail.organize` | Flagging, tagging, marking read, moving, archiving and snoozing messages; moving them to Trash and deleting them (always asked). Needs `mail.read`. Can be limited to some accounts. | `nail.mail.update`, `nail.mail.delete`; actions of [arrival hooks](hooks.md#on-arrival) |
| `mail.send` | Writing drafts and sending them (asks unless you allow it); with `calendar.write`, mailing invitations and answers. Can be limited to some accounts (of the sending identity). | `nail.mail.createDraft`, `nail.mail.updateDraft`, `nail.mail.send` |
| `calendar.write` | Creating and changing events, answering invitations, deleting events (always asked); listing the calendars it may write to. | `nail.calendar.calendars`, `createEvent`, `updateEvent`, `deleteEvent`, `respond` |
| `contacts.write` | Creating and changing contacts, deleting them (always asked); listing address books and finding a contact by address. | `nail.contacts.*` |
| `tasks.write` | Creating and changing tasks, deleting them (always asked); listing task lists. | `nail.tasks.*` |

## Mail hooks

| Permission | What it allows |
| --- | --- |
| `hooks.arrival` | Being called for each new message that matches its filter, after your own rules, and answering actions like a rule's (flag, tag, mark read, move, archive — they need `mail.organize`; never delete). Needs `mail.read`. Can be limited to some accounts. |
| `hooks.beforeSend` | Seeing every message you send (recipients, subject, text, attachment names) before it is queued, and adding `X-` headers or text, or stopping it with a reason — you can always **Send anyway**. Can be limited to some accounts. |
| `hooks.open` | Being called when you open a message that matches its filter, and showing banners and badges with it (it never changes the message). Needs `mail.read`. Can be limited to some accounts. |

See [mail hooks](hooks.md).

## Extensions

| Permission | What it allows |
| --- | --- |
| `composer` | Composer actions that insert or replace text in the draft you have open (you see the change and can undo it), with the text you selected; Markdown blocks it renders when you preview or send. See [composer](composer.md). |
| `attachments` | Previews of the attachment types it names, and entries in the attachment menu; it gets the content of the attachment you preview or pick (never of end-to-end encrypted mail). Can be limited to some accounts. See [attachments and links](attachments-and-links.md). |
| `background` | Running jobs on a schedule or when something happens (new mail, a sent message, a reminder, Nail starting), within a CPU quota. See [background jobs](background-jobs.md). |
| `mcp.expose` | Offering tools to the AI agents (MCP clients) you connected, under those clients' permissions. See [tools for AI agents](mcp-tools.md). |

Link actions need no permission: they get only the link you picked (and
the message it is in, when the plug-in may read it). The reader does not
offer link actions yet.

## Everything else

| Permission | What it allows | API |
| --- | --- | --- |
| `ui.dialogs` | Showing dialogs. | `nail.ui.showDialog` |
| `notifications` | Showing notifications, in Nail and (if the plug-in asks) as system notifications, which follow your quiet hours. | `nail.ui.notify` |
| `storage` | Keeping its own data on this device, within a size limit (5 MB unless you change it). | `nail.storage` |
| `network:<host>` | Connecting to that host, through Nail: `network:api.example.com` for one host, `network:*.example.com` for its subdomains (not `example.com` itself). | `nail.http.fetch` |

Commands, panels, settings, translations (`nail.l10n`) and the log need no
permission.

## What plug-ins can never do

- Read the content of end-to-end encrypted mail (OpenPGP, S/MIME), even
  with every permission.
- Delete anything without your confirmation in Nail, or have a permanent
  delete confirmed by anything but you.
- Keep a message from being sent: a before-send hook can stop it once,
  with a reason, and **Send anyway** sends it.
- Rewrite a message you read: open hooks add banners and badges next to
  it.
- Read files on your computer, start programs or open network connections
  themselves. Web requests go through Nail, only to the hosts listed, only
  over `https:`, and never with your cookies or passwords.
- See other plug-ins' data.
- Run code in Nail's window: plug-ins describe their dialogs, panels and
  previews as data (see the [component reference](components.md)), and
  Nail draws them; previews are never HTML.

More in [security](security.md).

## Limiting access to accounts

`mail.read`, `mail.read.body`, `mail.organize`, `mail.send`, the three
mail hooks and `attachments` can be granted for every account or only for
some. A plug-in limited to your work account gets `forbidden` for messages
of other accounts, its searches only return mail from the granted
accounts, and its hooks are not called for mail of other accounts.

## Changing permissions later

In the plug-in manager you can revoke a permission or limit it at any
time; the plug-in keeps running and its next call without the permission
fails. A new version of a plug-in that asks for more permissions shows the
install dialog again.

## Permissions that are not available yet

Manifests naming `mail.read.encrypted`, `contacts.read`, `tasks.read` or
`ui.webview` are refused ("The permission … is not available yet"); such plug-ins
need a later version of Nail. (The write permissions include what they
need to read: `tasks.write` can list task lists, `contacts.write` can find
a contact by address.)
