# Manifest reference (`manifest.json`)

Every plug-in has a `manifest.json` at the top of its package. Nail checks
it when you install or load the plug-in and lists every problem at once
(the install dialog and `nail plugin dev` show them). `npm run package`
runs a quick check of the same rules before packing.

```json
{
  "id": "follow-up",
  "name": "%plugin-name%",
  "version": "2.0.0",
  "apiVersion": "1",
  "main": "dist/index.js",
  "description": "%plugin-description%",
  "author": "Nail",
  "permissions": ["mail.read", "tasks.write", "ui.dialogs", "notifications", "storage", "background"],
  "contributes": {
    "commands": [
      { "id": "follow-up", "title": "%follow-up-title%", "icon": "flag", "contexts": ["list", "reader"] }
    ],
    "placements": [{ "toolbar": "reader", "after": "mail.forward", "command": "follow-up" }],
    "keys": [{ "key": "shift+f", "command": "follow-up", "context": "list" }],
    "panels": [{ "id": "due", "title": "%due-title%", "icon": "flag" }],
    "jobs": [{ "id": "remind", "title": "%remind-title%", "schedule": "@hourly", "events": ["app.started"] }],
    "settings": [
      { "type": "number", "key": "default-days", "title": "%setting-days-title%", "default": 1, "min": 0, "max": 365 }
    ],
    "locales": { "en": "locales/en.ftl" }
  }
}
```

## Top-level fields

| Field | Required | What it is |
| --- | --- | --- |
| `id` | yes | The plug-in's id: lower-case letters, digits and single hyphens, starting with a letter, at most 64 characters (`follow-up`). Unique on one Nail; it names the plug-in's commands and panels in `layout.kdl`. |
| `name` | yes | Display name, at most 100 characters (or a `%text-id%`). |
| `version` | yes | Semantic version: `1.0.0`, `2.1.0-beta.1`. |
| `apiVersion` | yes | The plug-in API it is written for: `"1"`. Nail supports the current and the previous major version. |
| `main` | yes | The bundled JavaScript file inside the package, ending in `.js` or `.mjs` (`dist/index.js`). |
| `description` | no | One paragraph, at most 1000 characters (or a `%text-id%`). |
| `author` | no | Who wrote it. |
| `homepage` | no | A web page about it; `https:` only, shown as text. |
| `permissions` | no | What it may do; see [permissions](permissions.md). Each at most once; `mail.read.body`, `mail.organize`, `hooks.arrival` and `hooks.open` need `mail.read`. |
| `contributes` | no | What it adds to Nail (below). |

Paths inside the package use `/`, letters, digits, `.`, `-` and `_`, and
cannot leave the package (`..`).

## Texts and translations

`name`, `description` and every `title` and `description` below may be a
text id between percent signs, `%follow-up-title%`. Nail looks it up in the
plug-in's own Fluent catalogs (`contributes.locales`) for your language,
then in English, and shows the id itself when neither has it. Other texts
are shown as written.

```json
"locales": { "en": "locales/en.ftl", "de": "locales/de.ftl" }
```

```ftl
# locales/en.ftl
follow-up-title = Follow up…
```

Keys of `locales` are language tags (`en`, `de`, `pt-BR`); values are
`.ftl` files in the package. At most 50. The plug-in's code reads the same
catalogs with `nail.l10n.t(id)` ([translations](localization.md)).

## `contributes.commands`

Named actions. A command becomes the action `plugin.<plugin-id>.<command>`
in `layout.kdl` (toolbars, menus, key bindings) and is run by the handler
registered with `ctx.commands.register`.

| Field | Required | What it is |
| --- | --- | --- |
| `id` | yes | The command's name (same rule as the plug-in id). |
| `title` | yes | The label, at most 100 characters (or `%text-id%`). |
| `icon` | no | An icon name of your theme's icon set (`flag`). |
| `description` | no | Tooltip text (or `%text-id%`). |
| `contexts` | no | Where it can run: `list`, `reader`, `composer`, `global`. Empty or missing: everywhere. |

At most 100 commands.

## `contributes.placements`

Where Nail suggests putting the plug-in's commands and panels. On install
Nail shows the suggested lines for `layout.kdl` and writes them only when
you confirm; afterwards they are ordinary lines you can move or delete.

Each placement has exactly one of `toolbar`, `menu` or `panel`, and at most
one of `after` or `before`:

| Kind | Example | Result in `layout.kdl` |
| --- | --- | --- |
| Toolbar item | `{ "toolbar": "reader", "command": "follow-up", "after": "mail.forward" }` | `item "plugin.follow-up.follow-up"` in `toolbar "reader"`, after `mail.forward` |
| Menu item | `{ "menu": "context:message-list.row", "command": "follow-up" }` | an item in that menu (`app` or `context:<part>`) |
| Panel | `{ "panel": "due", "after": "message-view" }` | `plugin-panel "follow-up.due"` after the first `message-view`; at the end of the main window when there is none |

`command` must name a command of the manifest, `panel` a panel of the
manifest. `after` / `before` name an action (toolbars, menus) or a widget
kind (panels). At most 100 placements.

## `contributes.keys`

Suggested key bindings, written like those in `layout.kdl`.

| Field | Required | What it is |
| --- | --- | --- |
| `key` | yes | The keys: `shift+f`, `ctrl+alt+r`, `g f` (a sequence). |
| `command` | yes | A command of the manifest. |
| `context` | no | Where the binding applies: `global` (default), `list`, `reader`, `composer`, `search`, `window`. |

At most 100 keys.

## `contributes.panels`

Panels the user can place in the layout with
`plugin-panel "<plugin-id>.<panel>"`. The plug-in draws them with
`ctx.panels.register`.

| Field | Required | What it is |
| --- | --- | --- |
| `id` | yes | The panel's name (same rule as the plug-in id). |
| `title` | yes | Its title (or `%text-id%`). |
| `icon` | no | An icon name. |

At most 50 panels.

## `contributes.settings`

A settings schema; Nail turns it into a settings page for the plug-in, and
the plug-in reads the values with `nail.settings.get(key)`.

| `type` | Extra fields | Value |
| --- | --- | --- |
| `string` | `default`, `multiline`, `maxLength` (at most 10 000) | text |
| `number` | `default` (0 or `min` when missing), `min`, `max` | whole number |
| `boolean` | `default` | `true` / `false` |
| `select` | `options` (`[{ "value": "a", "label": "A" }]`, labels may be `%text-id%`), `default` (the first option when missing) | one option's `value` |

Every setting has `key` (same rule as the plug-in id), `title` and an
optional `description`. At most 50 settings.

## `contributes.hooks`

Mail hooks, at most one of each kind; see [mail hooks](hooks.md). Each
needs its permission (`hooks.arrival`, `hooks.beforeSend`, `hooks.open`).

| Field | Required | What it is |
| --- | --- | --- |
| `hook` | yes | `arrival`, `beforeSend` or `open`. |
| `filter` | no | Only messages matching this query of the [search language](../search.md) (at most 500 characters); `arrival` and `open` only. |
| `timeoutMs` | no | The time budget of one call, 50–10 000 ms (default 2000 / 3000 / 1000). |

## `contributes.composerActions` and `contributes.markdownBlocks`

Need `composer`; see [composer](composer.md). At most 50 of each.

| Field | Required | What it is |
| --- | --- | --- |
| `id` | yes | The name (same rule as the plug-in id). A block's fence is `<plugin-id>.<id>`. |
| `title` | yes | The label in the composer (or `%text-id%`). |
| `icon` | no | Actions: an icon name. |
| `description` | no | Actions: a tooltip (or `%text-id%`). |
| `template` | no | Blocks: the text inserted as the block's content, at most 1000 characters (or `%text-id%`). |

## `contributes.attachmentPreviews` and `contributes.attachmentActions`

Need `attachments`; see [attachments and links](attachments-and-links.md).
At most 50 of each.

| Field | Required | What it is |
| --- | --- | --- |
| `id` | yes | The name. |
| `title` | yes | The label (or `%text-id%`). |
| `mimeTypes` | no | MIME types, lower case (`text/csv`, `image/*`); at most 20. |
| `extensions` | no | File extensions, lower case without the dot (`csv`); at most 20. A preview needs types or extensions; an action without either applies to every attachment. |
| `maxBytes` | no | Previews: the largest attachment it renders (at most 10 MB, the default). |
| `icon` | no | Actions: an icon name. |

## `contributes.linkActions`

Actions for links; no permission needed. At most 50. The reader does not
offer link actions yet, so users cannot pick them in this version (see
[attachments and links](attachments-and-links.md)).

| Field | Required | What it is |
| --- | --- | --- |
| `id` | yes | The name. |
| `title` | yes | The label (or `%text-id%`). |
| `icon` | no | An icon name. |
| `hosts` | no | Host patterns as for `network:` (`jira.example.com`, `*.example.com`); empty: every link. |

## `contributes.jobs`

Background jobs; need `background`; see [background jobs](background-jobs.md).
At most 10.

| Field | Required | What it is |
| --- | --- | --- |
| `id` | yes | The name. |
| `title` | no | What the plug-in manager calls it (or `%text-id%`). |
| `schedule` | one of the two | Five cron fields in local time or `@hourly` / `@daily` / `@weekly`; at most every 5 minutes. |
| `events` | one of the two | `message.received`, `message.sent`, `selection.changed`, `event.reminder`, `app.started`. |

## `contributes.mcpTools`

Tools for AI agents; need `mcp.expose`; see [tools for AI agents](mcp-tools.md).
At most 50.

| Field | Required | What it is |
| --- | --- | --- |
| `id` | yes | The name; agents see `<plugin-id>__<id>` (at most 64 characters in all). |
| `description` | yes | What it does, for the agent, at most 1000 characters (or `%text-id%`: agents get it in Nail's language). |
| `inputSchema` | no | The JSON Schema of the arguments: an object schema, at most 16 KB. |
| `scopes` | no | The MCP permissions a client needs to call it (default `["read"]`). |

## Problems Nail reports

Besides the rules above, Nail refuses a manifest when a contribution's
permission is missing ("… needs the permission …"), a permission another
one needs is missing, a name repeats, or an entry is invalid (a hook with a
filter where none is allowed, a schedule that runs too often, a preview
without types, a tool whose name is too long). `npm run package` and
`nail-plugin check` report the same problems before you pack.

## Limits

`manifest.json` may be at most 64 KB. The full list of fields and types is
in the [API reference](api-reference.md#manifest-types).
