# @nail/plugin-api

The SDK for Nail plug-ins (preview, M2d; ADR 0050 contract, WP63 ADR
`docs/adr/0053-plugin-sdk.md`). User documentation: `userDocs/en/plugins/`.

| Import | What it is |
| --- | --- |
| `@nail/plugin-api` | `definePlugin`, the `nail` wrapper, `isPluginApiError`, `toNodes`, `checkTree`, and every type: the contract types in `src/generated/` (ts-rs from `crates/nail-api`, `npm run gen:api`; never edit by hand) and the runtime API in `src/api.ts` (the host's global `nail`, `crates/nail-plugins/src/prelude.js`) |
| `@nail/plugin-api/jsx-runtime` | The JSX runtime (`jsxImportSource: "@nail/plugin-api"`) producing `PluginUiNode` trees |
| `@nail/plugin-api/globals` | Types of the sandbox globals (`console`, `setTimeout`, `clearTimeout`, `queueMicrotask`) |
| `@nail/plugin-api/testing` | `createTestHost`: an in-memory host with the real permission rules, for plug-in unit tests |
| `@nail/plugin-api/tools` | Node-side: `checkManifest`, `packPlugin` (`.nailplugin`, deterministic zip), `readZip` / `createZip` |
| bin `nail-plugin` | `nail-plugin check [dir]`, `nail-plugin pack [dir] [--out FILE]` |

```
npm test -w packages/plugin-api     # Vitest
npm run lint -w packages/plugin-api # tsc (also with only the sandbox's globals) + Prettier
npm run docs -w packages/plugin-api # regenerate the reference parts of userDocs/en/plugins/
```

The package is consumed as TypeScript source (bundlers and `tsc` with
`moduleResolution: "bundler"`); it is not published.
