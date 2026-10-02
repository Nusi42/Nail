# __PLUGIN_NAME__

A Nail plug-in (plug-in API, `apiVersion` 1).

| Command             | What it does                                                                         |
| ------------------- | ------------------------------------------------------------------------------------ |
| `npm install`       | Installs the SDK (`@nail/plugin-api`), esbuild and TypeScript.                       |
| `npm run build`     | Bundles `src/index.tsx` into `dist/index.js`.                                        |
| `npm run watch`     | Rebuilds on every change; run `nail plugin dev .` next to it for hot reload in Nail. |
| `npm run typecheck` | Type-checks the sources.                                                             |
| `npm run package`   | Type-checks, builds and packs `dist/__PLUGIN_ID__.nailplugin`.                       |

Install the `.nailplugin` file in Nail from the plug-in manager ("Install from file").

- `manifest.json` — id, name, permissions and what the plug-in adds to Nail
  (commands, hooks, composer actions, attachment previews, …).
- `src/index.tsx` — the code; JSX builds dialogs, panels and previews from Nail's components.
- `locales/en.ftl` — texts the manifest references as `%id%` and the code reads with
  `nail.l10n.t(id)`; add `locales/<language>.ftl` files for translations.

Test the plug-in with the SDK's test host (`@nail/plugin-api/testing`), which runs
commands, hooks and previews with Nail's permission rules.

See Nail's user documentation, "Plug-ins", for the manifest, the permissions,
the extension points, the API and the components.
