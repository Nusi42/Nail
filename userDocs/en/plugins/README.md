# Plug-ins (preview)

Plug-ins add commands, dialogs and panels to Nail. They run inside Nail in a
sandbox, one per plug-in, and can only do what you allow when you install
them. Their buttons, menus and panels are ordinary lines in `layout.kdl`
that you can move or delete, and your theme styles them like the rest of
Nail.

This section grows with the plug-in preview (milestone M2): getting started,
writing a plug-in, the manifest, the API and the permissions.

## Permissions

A plug-in lists the permissions it needs; the install dialog shows them and
you can revoke them later in the plug-in manager.

| Permission | What it allows |
| --- | --- |
| `mail.read` | Reading your accounts, folders, senders and subjects (not message text). Can be limited to some accounts. |
| `mail.read.body` | Reading the text of messages too (never of end-to-end encrypted mail). Can be limited to some accounts. |
| `calendar.read` | Reserved for calendars; it allows nothing yet. |
| `ui.dialogs` | Showing dialogs. |
| `notifications` | Showing notifications. |
| `storage` | Keeping its own data on this device, within a size limit (5 MB). |
| `network:<host>` | Connecting to that host (for example `network:api.example.com`, or `network:*.example.com` for its subdomains), through Nail. |

Plug-ins never get access to your files, other programs or the network
beyond the hosts they list.

## Plug-in panels in `layout.kdl`

A plug-in panel is placed like any widget, with the plug-in's id and the
panel's name:

```kdl
split "horizontal" sizes="240px 1fr 280px" {
    message-list
    message-view
    plugin-panel "follow-up.due"
}
```
