# __PLUGIN_NAME__

A Nail plug-in (preview API, `apiVersion` 1).

| Command             | What it does                                                                         |
| ------------------- | ------------------------------------------------------------------------------------ |
| `npm install`       | Installs the SDK (`@nail/plugin-api`), esbuild and TypeScript.                       |
| `npm run build`     | Bundles `src/index.tsx` into `dist/index.js`.                                        |
| `npm run watch`     | Rebuilds on every change; run `nail plugin dev .` next to it for hot reload in Nail. |
| `npm run typecheck` | Type-checks the sources.                                                             |
| `npm run package`   | Type-checks, builds and packs `dist/__PLUGIN_ID__.nailplugin`.                       |

Install the `.nailplugin` file in Nail from the plug-in manager ("Install from file").

- `manifest.json` — id, name, permissions, commands, toolbar placements, locales.
- `src/index.tsx` — the code; JSX builds dialogs and panels from Nail's components.
- `locales/en.ftl` — texts that `manifest.json` references as `%id%`.

See Nail's user documentation, "Plug-ins", for the manifest, the permissions,
the API and the components.
