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

A plug-in comes as a `.nailplugin` file. Open **Settings → Plug-ins** and
choose **Install a plug-in…**, then give the path of the file on this
computer or an `https:` address, and choose **Check the plug-in**.

Before anything is installed Nail shows the plug-in's name, version and
author, what it adds (commands, panels), the package's size and fingerprint
(the start of its SHA-256; the plug-in's author can tell you the full
value), problems that prevent installing it, and every permission it asks
for with **Allow** / **Deny**. Denying "Read your mail" also denies reading
message text. The mail permissions can be limited to some accounts
("Only these accounts"; at least one). **Turn it on right away** starts it
after installing. **Install** installs exactly the package that was
checked: if the file changed in between, Nail says so and you check it
again.

Then Nail proposes where its buttons, menu items, shortcuts and panels go
in your `layout.kdl`: every proposed item with a check box, all ticked.
**Add … to the layout** writes the ticked items (the previous `layout.kdl`
is backed up, and **Undo** restores it); **Not now** writes nothing. A
shortcut the plug-in suggests is never added when you (or Nail's defaults)
already use that key there. You can come back to this step later with
**Place in layout** in the plug-in manager.

## The plug-in manager

**Settings → Plug-ins** lists every installed plug-in with its version,
where it came from and its state: *Running*, *Off*, *Not running*,
*Stopped after a problem* (with the reason) or *Not compatible with this
version of Nail*. For each plug-in:

- the switch turns it on and off (turning it on again also restarts one
  that stopped after a problem);
- **Permissions** changes or revokes what it may do (from its next call
  on);
- **Settings** is the settings page its manifest describes;
- **Log** shows its log: its own lines, Nail's lines about it (started,
  stopped, reloaded) and every refused call, filtered by level, and
  **Clear log**;
- **Place in layout** proposes its buttons and panels again;
- **Remove** uninstalls it after a confirmation. Its settings, data and log
  are deleted; the lines it added to `layout.kdl` stay until you delete
  them (a button of a missing plug-in stays disabled).

A plug-in that throws an error, runs too long or uses too much memory is
stopped at once and marked *Stopped after a problem*; a message says why.
Nail itself and your other plug-ins keep running.

Plug-in dialogs always say which plug-in they come from ("from Follow
up"). Plug-in notifications appear in Nail; a plug-in can also ask for a
system notification, which follows your notification settings (switch and
quiet hours).

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

## Styling plug-ins in `theme.kdl`

Everything a plug-in shows is drawn by Nail and styled by your theme. Each
element has a part named `plugin.<element>` (`plugin.dialog`,
`plugin.button`, `plugin.text`, `plugin.notification`, …; see the
[component reference](components.md)) and carries the plug-in's id, so a
rule can style every plug-in or one of them:

```kdl
part "plugin.button" { border-radius "$radius.md"; }
part "plugin.button[plugin=\"follow-up\"]" { color "$color.accent"; }
part "plugin.follow-up.button" { font-weight "$font-weight.bold"; }
```

`plugin.<plugin-id>.<element>` is short for the second form. The plug-in
manager, the install dialog and the log use the parts `plugin-manager.*`,
`plugin-install.*`, `plugin-permissions.*`, `plugin-placements.*`,
`plugin-settings.*` and `plugin-logs.*`.

## Remote access

Plug-ins run in the Nail that owns your mail. A browser connected through
remote access uses that Nail's plug-ins, and only when the signed-in user
may read every account; installing, switching and configuring plug-ins
needs the administrator right. Plug-in folders for development can only be
loaded on the desktop.
