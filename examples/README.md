# Examples

Example Nail plug-ins (M2d preview, WP63 ADR `docs/adr/0053-plugin-sdk.md`;
full API M4a, WP103 ADR `docs/adr/0082-plugin-sdk-m4a.md`). Each one is built with
the starter template's scripts (`scripts/build.mjs`, `scripts/pack.mjs`,
`tsconfig.json`, identical to `packages/create-nail-plugin/template/`) and its
runtime texts come from its own Fluent catalogs through `nail.l10n`.

## `follow-up`

The design's "Follow up" example: a **Follow up…** button in the reader
toolbar (Shift+F in the message list) opens a dialog with a due date, a task
list and a note, and creates a task linked to the message
(`nail.tasks.create`, `links: [message.uri]`). The plug-in remembers the
follow-up in its storage; its `due` panel (`plugin-panel "follow-up.due"`)
lists them, and its `remind` background job (hourly and when Nail starts)
shows a notification when one is due. A denied or expired confirmation (when
the user makes task writes ask) creates nothing. Permissions: `mail.read`,
`tasks.write`, `ui.dialogs`, `notifications`, `storage`, `background`.

## `attachment-check`

A before-send hook: a message whose subject or own text (not the signature,
not the quoted message unless the setting says so) mentions an attachment
("attached", "enclosed", „anbei“) but has none is stopped with a reason;
Nail names the plug-in and offers **Send anyway**. The keyword lists are
per language in `locales/en.ftl` and `locales/de.ftl` (`attachment-keywords`,
read with `nail.l10n.t`), plus the user's own words from the settings.
Permission: `hooks.beforeSend`.

## `csv-preview`

An attachment preview renderer: CSV and TSV attachments (up to 2 MB) are
shown as a `<table>` component — never HTML. UTF-8 or Windows-1252, comma /
semicolon / tab / bar detected from the first line, quoted fields per
RFC 4180, the first row as the header (setting), at most 499 rows (setting).
Permission: `attachments`.

## Building and packing

```
cd examples/<name>
npm run package      # type-check, bundle, pack → dist/<name>.nailplugin
```

or, without npm scripts (for example in e2e setup; `--out` puts the
package anywhere, outside the repository):

```
node examples/<name>/scripts/build.mjs
node examples/<name>/scripts/pack.mjs --out <dir>/<name>.nailplugin
```

The folders are not npm workspaces; `@nail/plugin-api`, esbuild and
TypeScript resolve from the repository's `node_modules`. `dist/` is
gitignored and never committed. The tests live in
`packages/create-nail-plugin/test/` (`examples.test.ts` for what every
example must do, `example.test.ts`, `attachment-check.test.ts`,
`csv-preview.test.ts` for their behaviour); they copy each example to a temp
folder and build it there, so nothing is written into the repository.
