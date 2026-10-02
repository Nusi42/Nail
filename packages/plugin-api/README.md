# @nail/plugin-api

The SDK for Nail plug-ins (M2d preview, ADR 0050 contract / ADR 0053 SDK;
full API M4a, ADR 0079 contract / `docs/adr/0082-plugin-sdk-m4a.md`). User
documentation: `userDocs/en/plugins/`.

| Import | What it is |
| --- | --- |
| `@nail/plugin-api` | `definePlugin` (with declared handlers: `commands`, `panels`, `hooks`, `composer`, `attachments`, `links`, `jobs`, `mcpTools`), the `nail` wrapper, hook / preview helpers (`veto`, `addContent`, `banner`, `badge`, `previewTree`, `previewImage`, `isDone`, `createdId`, and checks mirroring the host's), bytes helpers (`decodeText`, `encodeUtf8`, `toBase64`, …), `isPluginApiError`, `toNodes`, `checkTree`, and every type: the contract types in `src/generated/` (ts-rs from `crates/nail-api`, `npm run gen:api`; never edit by hand) and the runtime API in `src/api.ts` (the host's global `nail`, `crates/nail-plugins/src/prelude.js`, mirrored by hand) |
| `@nail/plugin-api/jsx-runtime` | The JSX runtime (`jsxImportSource: "@nail/plugin-api"`) producing `PluginUiNode` trees |
| `@nail/plugin-api/globals` | Types of the sandbox globals (`console`, `setTimeout`, `clearTimeout`, `queueMicrotask`) |
| `@nail/plugin-api/testing` | `createTestHost`: an in-memory host with the real permission rules, write policies and scripted confirmations, hook budgets and answer checks, previews, jobs, MCP tools, `nail.l10n`; `FluentCatalog` / `createLocalizer` |
| `@nail/plugin-api/tools` | Node-side: `checkManifest` (all M4a rules), `packPlugin` (`.nailplugin`, deterministic zip), `readZip` / `createZip`, `parseCron` / `nextCronRun` / `cronMatches`, `loadCatalogs` |
| bin `nail-plugin` | `nail-plugin check [dir]`, `nail-plugin pack [dir] [--out FILE]` |

```
npm test -w packages/plugin-api     # Vitest
npm run lint -w packages/plugin-api # tsc (also with only the sandbox's globals) + Prettier
npm run docs -w packages/plugin-api # regenerate the reference parts of userDocs/en/plugins/
```

The package is consumed as TypeScript source (bundlers and `tsc` with
`moduleResolution: "bundler"`); it is not published.
