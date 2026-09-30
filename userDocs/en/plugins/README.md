# Plug-ins (preview)

Plug-ins add commands, dialogs and panels to Nail. They run inside Nail in a
sandbox, one per plug-in, and can only do what you allow when you install
them. Their buttons, menus and panels are ordinary lines in `layout.kdl`
that you can move or delete, and your theme styles them like the rest of
Nail.

In this preview plug-ins can **read** mail (if you allow it), show dialogs
and notifications, keep their own data and reach the web hosts they name.
They cannot change, move, delete or send mail, and they never see the
content of end-to-end encrypted messages.

| Page | What it covers |
| --- | --- |
| [Permissions](permissions.md) | What each permission allows, limiting mail access to accounts, revoking |
| [Getting started](getting-started.md) | Writing a plug-in: the starter template, building, packing, installing, `nail plugin dev`, the "Follow up" example |
| [Manifest reference](manifest.md) | `manifest.json`: id, version, permissions, commands, placements, keys, panels, settings, translations |
| [API reference](api-reference.md) | `definePlugin`, the `nail` object (mail, dialogs, notifications, storage, HTTP, settings, log), errors, every type |
| [Component reference](components.md) | The building blocks of dialogs and panels, JSX, the values inputs report |

## Installing a plug-in

A plug-in comes as a `.nailplugin` file. Install it from the plug-in
manager (from a file on this computer, or from an `https:` address).
Before anything is installed Nail shows the plug-in's name, version, the
package's fingerprint (SHA-256) and every permission it asks for; you grant
all of them or limit mail access to some accounts. Nail then suggests where
its buttons and panels go and shows the changes to `layout.kdl` before
writing them.

In the plug-in manager you can switch a plug-in off and on, change or
revoke its permissions, read its log and uninstall it. Uninstalling removes
its data; the lines it added to `layout.kdl` stay until you delete them.

A plug-in that throws an error or runs too long is stopped and marked in
the plug-in manager; Nail itself keeps running.

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

Plug-in commands are actions named `plugin.<plugin-id>.<command>`, used like
built-in actions in toolbars, menus and key bindings:

```kdl
toolbar "reader" { item "mail.reply"; item "mail.forward"; item "plugin.follow-up.follow-up" }
```

## Remote access

Plug-ins run in the Nail that owns your mail. A browser connected through
remote access uses that Nail's plug-ins, and only when the signed-in user
may read every account; installing, switching and configuring plug-ins
needs the administrator right. Plug-in folders for development can only be
loaded on the desktop.
