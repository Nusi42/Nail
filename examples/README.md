# Examples

Example Nail plug-ins (M2d preview; WP63 ADR `docs/adr/0053-plugin-sdk.md`).

## `follow-up`

The design's "Follow up" example adapted to the read-only preview API: a
**Follow up…** button in the reader toolbar (Shift+F in the message list)
opens a dialog with a due date and a note, stores the reminder in plug-in
storage and shows a notification. Its `due` panel
(`plugin-panel "follow-up.due"`) lists the reminders. Permissions:
`mail.read`, `ui.dialogs`, `storage`, `notifications`.

```
cd examples/follow-up
npm run package      # type-check, bundle, pack → dist/follow-up.nailplugin
```

or, without npm scripts (for example in e2e setup; `--out` puts the
package anywhere):

```
node examples/follow-up/scripts/build.mjs
node examples/follow-up/scripts/pack.mjs --out <dir>/follow-up.nailplugin
```

The folder is not an npm workspace; `@nail/plugin-api`, esbuild and
TypeScript resolve from the repository's `node_modules`. `dist/` is
gitignored and never committed. The tests live in
`packages/create-nail-plugin/test/example.test.ts`.
