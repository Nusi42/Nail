# Plug-ins

Plug-ins add commands, dialogs and panels to Nail, check and sort mail,
extend the composer, preview attachments, run small jobs in the
background and offer tools to AI agents. They run inside Nail in a
sandbox, one per plug-in, and can only do what you allow when you install
them. Their buttons, menus and panels are ordinary lines in `layout.kdl`
that you can move or delete, and your theme styles them like the rest of
Nail.

A plug-in can **read** mail (if you allow it), **change** your data —
flag, tag, move or delete mail, write drafts and send them, change events,
contacts and tasks — only with the matching permission, and every change
goes through the same undo history and log as your own. Deleting always
asks you first, and sending asks unless you allowed it for that plug-in.
Plug-ins never see the content of end-to-end encrypted messages.

| Page | What it covers |
| --- | --- |
| [Permissions](permissions.md) | What each permission allows, limiting it to accounts, revoking |
| [Security](security.md) | What the sandbox prevents, what to check before installing, how confirmations protect you |
| [Getting started](getting-started.md) | Writing a plug-in: the templates, building, packing, installing, `nail plugin dev`, testing, the examples |
| [Manifest reference](manifest.md) | `manifest.json`: id, version, permissions and every contribution |
| [Writing data](writing-data.md) | Changing mail, drafts, sending, events, contacts and tasks; confirmations and their results |
| [Mail hooks](hooks.md) | On arrival, before send (with a veto the user can override), on open (banners and badges) |
| [Composer](composer.md) | Composer actions that insert or transform text; Markdown blocks rendered at send time |
| [Attachments and links](attachments-and-links.md) | Attachment previews (components or images, never HTML), attachment and link actions |
| [Background jobs](background-jobs.md) | Schedules, event subscriptions, time limits and the CPU quota |
| [Tools for AI agents](mcp-tools.md) | Offering plug-in tools to MCP clients (`mcp.expose`) |
| [Translations](localization.md) | The plug-in's own Fluent catalogs, `%id%` texts and `nail.l10n` |
| [API reference](api-reference.md) | `definePlugin`, the `nail` object, every handler, every type |
| [Component reference](components.md) | The building blocks of dialogs, panels and previews, JSX, the values inputs report |

## Installing a plug-in

A plug-in comes as a `.nailplugin` file. Open **Settings → Plug-ins** and
choose **Install a plug-in…**, then give the path of the file on this
computer or an `https:` address, and choose **Check the plug-in**.

Before anything is installed Nail shows the plug-in's name, version and
author, what it adds (commands, panels, mail hooks, composer actions,
attachment previews, background jobs, tools for AI agents), the package's
size and fingerprint (the start of its SHA-256; the plug-in's author can
tell you the full value), problems that prevent installing it, and every
permission it asks for with **Allow** / **Deny**. Permissions that let the
plug-in change your data are marked *Can change your data*. Denying "Read
your mail" also denies reading message text. The mail permissions, the
mail hooks and the attachment permission can be limited to some accounts
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
- **Confirmations** chooses what it has to ask you about before it changes
  something ([writing data](writing-data.md#confirmations));
- **Activity** shows its mail hooks (calls, failures, calls that were too
  slow, mail it stopped, time per call), its background jobs (schedule,
  last and next run, failures, **Run now**) and the CPU time it used in
  the last hour;
- **Settings** is the settings page its manifest describes;
- **Log** shows its log: its own lines, Nail's lines about it (started,
  stopped, reloaded) and every refused call, filtered by level, and
  **Clear log**;
- **Place in layout** proposes its buttons and panels again;
- **Remove** uninstalls it after a confirmation. Its settings, data and log
  are deleted; the lines it added to `layout.kdl` stay until you delete
  them (a button of a missing plug-in stays disabled).

A plug-in that throws an error in a command, a panel or while starting, or
that runs too long or uses too much memory, is stopped at once and marked
*Stopped after a problem*; a message says why. A mail hook, preview or
background job that fails is only skipped and counted under **Activity**:
mail still arrives, messages are still sent, the reader still opens.
Nail itself and your other plug-ins keep running.

Plug-in dialogs, banners, badges, previews and confirmations always say
which plug-in they come from ("from Follow up"). Plug-in notifications
appear in Nail; a plug-in can also ask for a system notification, which
follows your notification settings (switch and quiet hours).

## Where plug-ins show up

- **Toolbars, menus and keys**: plug-in commands, placed in `layout.kdl`.
- **Panels**: `plugin-panel "<plugin-id>.<panel>"` in `layout.kdl`.
- **The reader**: banners and badges of [open hooks](hooks.md#on-open),
  previews of attachments, and plug-in entries in the attachment and link
  menus ([attachments and links](attachments-and-links.md)).
- **The composer**: plug-in actions in the composer's toolbar and menu,
  and Markdown blocks in the block menu ([composer](composer.md)).
- **When you send**: a [before-send hook](hooks.md#before-send) may stop a
  message with a reason; the dialog names the plug-in and offers **Send
  anyway**.
- **Confirmations**: changes that need your approval wait in
  **Confirmations** with the plug-in's name, what would change and
  **Approve** / **Deny**.
- **AI agents**: tools a plug-in [offers to MCP clients](mcp-tools.md)
  appear next to Nail's own tools, under the clients' permissions.

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
may read every account; installing, switching and configuring plug-ins,
their confirmations and their activity need the administrator right.
Plug-in folders for development can only be loaded on the desktop.
