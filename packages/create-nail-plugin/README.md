# create-nail-plugin

Starter template for Nail plug-ins (the `npm create nail-plugin`
equivalent inside this repository; WP63 ADR `docs/adr/0053-plugin-sdk.md`).

```
node packages/create-nail-plugin/index.mjs <dir> [--id ID] [--name NAME] [--sdk SPEC] [--force]
```

Creates `<dir>` from `template/`: `manifest.json`, `src/index.tsx`,
`tsconfig.json`, `package.json` (scripts `build`, `watch`, `typecheck`,
`package`), `scripts/build.mjs` (esbuild bundle), `scripts/pack.mjs`
(`dist/<id>.nailplugin` via `@nail/plugin-api/tools`), `locales/en.ftl`,
`README.md` and `.gitignore`. `--sdk` is the version spec written for
`@nail/plugin-api`; the default is a `file:` path to `packages/plugin-api`.

Tests (`npm test -w packages/create-nail-plugin`) scaffold into a temp
directory, link `node_modules` to this repository's SDK, esbuild and
TypeScript, then type-check, build, pack and run the plug-in against the
SDK's test host. They also build and run `examples/follow-up`, whose
scripts must stay identical to the template's.
