# create-nail-plugin

Starter templates for Nail plug-ins (the `npm create nail-plugin`
equivalent inside this repository; WP63 ADR `docs/adr/0053-plugin-sdk.md`,
M4a templates `docs/adr/0082-plugin-sdk-m4a.md`).

```
node packages/create-nail-plugin/index.mjs <dir> [--template NAME] [--id ID] [--name NAME] [--sdk SPEC] [--force]
```

Creates `<dir>` from `template/`: `manifest.json`, `src/index.tsx`,
`tsconfig.json`, `package.json` (scripts `build`, `watch`, `typecheck`,
`package`), `scripts/build.mjs` (esbuild bundle), `scripts/pack.mjs`
(`dist/<id>.nailplugin` via `@nail/plugin-api/tools`), `locales/en.ftl`,
`README.md` and `.gitignore`. `--template` picks the starter:

| Template | Files from | What it starts with |
| --- | --- | --- |
| `command` (default) | `template/` | A command with a dialog and a notification |
| `hook` | `templates/hook/` | A before-send check and an open hook badge |
| `composer` | `templates/composer/` | Composer actions and a Markdown block |
| `attachment` | `templates/attachment/` | An attachment preview and an attachment action |

The other templates replace `manifest.json`, `src/index.tsx` and
`locales/en.ftl`; the rest is shared. `--sdk` is the version spec written for
`@nail/plugin-api`; the default is a `file:` path to `packages/plugin-api`.

Tests (`npm test -w packages/create-nail-plugin`) scaffold every template
into a temp directory, link `node_modules` to this repository's SDK,
esbuild and TypeScript, then type-check, build, pack and run the plug-in
against the SDK's test host. They also build, pack and run the examples in
`examples/` (copied to temp folders), whose scripts must stay identical to
the template's.
