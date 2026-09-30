# Plug-in permissions

A plug-in lists the permissions it needs in its manifest. The install
dialog shows them, and the plug-in can use only what you granted. Every
call a plug-in makes is checked; a call without its permission fails with
the error code `forbidden` and is written to the plug-in's log.

| Permission | What it allows | API |
| --- | --- | --- |
| `mail.read` | Reading your accounts, folders and message headers: subject, sender, recipients, date, flags, tags. Not the message text: searches look at these header fields only. Can be limited to some accounts. | `nail.mail.get`, `nail.mail.search`, `nail.mail.thread` |
| `mail.read.body` | Reading the text of messages too (plain text, cleaned-up HTML, attachment names and sizes, never attachment content), and searching it. Needs `mail.read`. Never the content of end-to-end encrypted mail: while you let Nail index decrypted mail for searching, plug-in searches leave encrypted mail out. Can be limited to some accounts. | `nail.mail.getBody`, `nail.mail.search` |
| `calendar.read` | Reserved for calendars; it allows nothing yet. | — |
| `ui.dialogs` | Showing dialogs. | `nail.ui.showDialog` |
| `notifications` | Showing notifications, in Nail and (if the plug-in asks) as system notifications, which follow your quiet hours. | `nail.ui.notify` |
| `storage` | Keeping its own data on this device, within a size limit (5 MB unless you change it). | `nail.storage` |
| `network:<host>` | Connecting to that host, through Nail: `network:api.example.com` for one host, `network:*.example.com` for its subdomains (not `example.com` itself). | `nail.http.fetch` |

Commands, panels, settings and the log need no permission.

## What plug-ins can never do

- Change, move, delete or send mail (that comes later, with its own
  permissions).
- Read the content of end-to-end encrypted mail (OpenPGP, S/MIME).
- Read files on your computer, start programs or open network connections
  themselves. Web requests go through Nail, only to the hosts listed, only
  over `https:`, and never with your cookies or passwords.
- See other plug-ins' data.
- Run code in Nail's window: plug-ins describe their dialogs and panels as
  data (see the [component reference](components.md)), and Nail draws them.

## Limiting mail access to accounts

`mail.read` and `mail.read.body` can be granted for every account or only
for some. A plug-in limited to your work account gets `forbidden` for
messages of other accounts, and its searches only return mail from the
granted accounts.

## Changing permissions later

In the plug-in manager you can revoke a permission or limit it at any
time; the plug-in keeps running and its next call without the permission
fails. A new version of a plug-in that asks for more permissions shows the
install dialog again.

## Permissions that are not in the preview

Manifests naming permissions of the full plug-in API (for example
`mail.send`, `mail.organize`, `tasks.write`, `hooks.arrival`) are refused
with "permission not in the preview"; such plug-ins need a later version of
Nail.
