# Writing a plug-in: getting started

A Nail plug-in is a small JavaScript program plus a `manifest.json`. You
write it in TypeScript (or JavaScript) with the `@nail/plugin-api` package,
bundle it into one file with esbuild, and pack it into a `.nailplugin`
file. Nail runs it in a sandbox (QuickJS) without Node.js or browser APIs:
everything a plug-in does goes through the `nail` object, and each
capability needs a [permission](permissions.md).

You need Node.js 24 or newer.

## 1. Create a plug-in from the template

In a checkout of Nail's repository:

```sh
node packages/create-nail-plugin/index.mjs ../my-plugin --name "My plug-in"
```

The id is taken from the folder name (`my-plugin`) unless you pass
`--id`. The new folder contains:

| File | Purpose |
| --- | --- |
| `manifest.json` | Id, name, version, permissions, commands, toolbar placements, translations ([manifest reference](manifest.md)) |
| `src/index.tsx` | The plug-in's code |
| `locales/en.ftl` | Texts the manifest refers to as `%id%` |
| `scripts/build.mjs` | Bundles `src/index.tsx` into `dist/index.js` with esbuild |
| `scripts/pack.mjs` | Packs `dist/<id>.nailplugin` |
| `tsconfig.json` | TypeScript settings: JSX through `@nail/plugin-api`, only the sandbox's globals |
| `package.json` | The scripts below; `@nail/plugin-api` points at the SDK in the repository |

Then:

```sh
cd ../my-plugin
npm install
npm run package
```

`npm run package` type-checks, builds and writes
`dist/my-plugin.nailplugin`. The other scripts are `npm run build`,
`npm run watch` (rebuild on every change) and `npm run typecheck`.

## 2. The code

```tsx
import { definePlugin, nail } from "@nail/plugin-api";

export default definePlugin({
  activate(ctx) {
    ctx.commands.register("hello", async (command) => {
      const result = await nail.ui.showDialog({
        title: "Hello",
        body: (
          <stack>
            <text>Messages selected: {command.selection.messageIds.length}</text>
            <input id="greeting" label="Your greeting" default="Hi" required />
          </stack>
        ),
        buttons: [
          { id: "ok", label: "Say it", primary: true },
          { id: "cancel", label: "Cancel" },
        ],
      });
      if (result.button !== "ok") return;
      await nail.ui.notify({ title: String(result.values.greeting), level: "success" });
    });
  },
});
```

- `definePlugin` declares the plug-in; `activate` runs once when it starts.
- `ctx.commands.register` connects a command of the manifest to its code.
  The handler gets the [command context](api-reference.md#command-context-and-selection):
  where it was run and which messages are selected.
- Dialogs and panels are written in JSX with Nail's
  [components](components.md) (`<text>`, `<input>`, `<date>`, `<stack>` …).
  They are data, not HTML: Nail draws them in its own style.
- Calls to Nail return promises. An error you do not catch stops the
  plug-in (it is marked as crashed in the plug-in manager), so catch the
  errors you expect, for example a missing permission:

  ```ts
  import { isPluginApiError } from "@nail/plugin-api";
  try {
    const body = await nail.mail.getBody(id);
  } catch (error) {
    if (!isPluginApiError(error, "forbidden")) throw error;
  }
  ```

The full API is in the [API reference](api-reference.md).

## 3. Try it: `nail plugin dev`

While you work, load the folder unpacked instead of installing a package:

```sh
npm run watch            # in one terminal: rebuild on every change
nail plugin dev .        # in another: load the folder into the running Nail
```

`nail plugin dev` loads the folder into the Nail that runs with its web UI
(`nail serve`): it finds it through the data directory, or pass
`--data-dir <dir>` or `--url <the address nail serve printed>`. Every
permission the manifest declares is granted, for all accounts. Nail
reloads the plug-in whenever a file in the folder changes (`node_modules`
and folders starting with `.` are ignored; a manifest with problems is
reported and the plug-in stays stopped until it is fixed), and the
terminal shows its log, errors and every API call with its outcome
(`console.log` and `nail.log.*` write there). Ctrl+C closes the console;
the plug-in stays loaded. In the desktop app, use **Load a development
folder…** in Settings → Plug-ins instead and read the log there.

## 4. Install the package

Install `dist/<id>.nailplugin` from Settings → Plug-ins ("Install a
plug-in…"). Nail shows the permissions and the package's SHA-256 fingerprint
(`npm run package` prints it too), then suggests where the plug-in's buttons
and panels go in `layout.kdl`.

## The sandbox and its limits

A plug-in runs in its own QuickJS runtime inside Nail. It has the
JavaScript language, `nail`, `console`, `setTimeout` / `clearTimeout` and
`queueMicrotask` — no Node.js or browser APIs, no files, processes or
sockets. Imports work only relative to the plug-in's own files
(`./util.js`); bundle everything else (the template's esbuild build does).

| Limit | Value |
| --- | --- |
| CPU time per call (a command, a dialog answer, a panel render, a timer, a resolved API call) | 1 s |
| CPU time while starting (`activate`) / time for `activate` to finish | 2 s / 30 s |
| Memory | 64 MiB (deep recursion throws a `RangeError` you can catch) |
| Pending API calls | 64 |
| Open dialogs | 5 |
| Timers | 100 |
| Notifications | 20 per minute |
| Log lines | 200 per call |
| `nail.http.fetch` | 1 MiB request body, 5 MiB response, text bodies |

**Crashes.** A plug-in that exceeds its CPU time or memory, or whose
command, panel, dialog action, timer or `activate` throws (or returns a
rejected promise), is stopped at once and marked in the plug-in manager
until the user turns it on again. A rejected promise nobody handles is
only logged. Catch the errors you expect: every refused call rejects with a
`NailError` whose `code` says why (`forbidden`, `not_found`, `too_large`,
`rate_limited`, …).

**`nail.http.fetch`** reaches only the hosts of the plug-in's `network:`
permissions, over `https:` on the standard port (`http:` and other ports
only for `127.0.0.1` when the permission names that address). Nail makes
the request: redirects are checked against the same hosts, host names
never lead to local or private network addresses, and the headers `Host`,
`Cookie`, `Content-Length`, connection headers and `Proxy-*` are refused.
Methods: GET, HEAD, POST, PUT, PATCH, DELETE.

## Testing a plug-in

`@nail/plugin-api/testing` has an in-memory stand-in for Nail with the same
permission rules. With Vitest, for example:

```ts
import { createTestHost } from "@nail/plugin-api/testing";
import manifest from "../manifest.json";
import plugin from "../src/index";

const host = createTestHost({
  manifest,
  dialog: () => ({ button: "ok", values: { greeting: "Hi" } }),
});
const restore = host.install();
await plugin.activate(host.activationContext());
await host.runCommand("hello", { selection: { messageIds: ["m1"] } });
console.log(host.dialogs, host.notifications);
restore();
```

## The "Follow up" example

`examples/follow-up` in the repository is a complete plug-in: a **Follow
up…** button in the reader toolbar (and Shift+F in the message list) opens a
dialog with a due date and a note, keeps the reminder in the plug-in's
storage and confirms it with a notification. Its **Follow-ups** panel
(`plugin-panel "follow-up.due"`) lists every reminder. It asks for
`mail.read`, `ui.dialogs`, `storage` and `notifications`, and has a setting
for how many days ahead the dialog suggests. Build it with
`npm run package` in its folder.

## Limits

| What | Limit |
| --- | --- |
| Package | 20 MB |
| `manifest.json` | 64 KB |
| Storage | 5 MB per plug-in (keys 256 bytes, values 1 MB) |
| Dialogs and panels | 500 components, nested 8 deep, 500 rows / options / messages, images 512 KB |
| Log | the last 2000 lines |

Plug-ins that run too long or use too much memory are stopped.
