# Feature slots

Parallel work packages each own one directory here, so no two packages edit
the same UI file (M1b: ADR 0022; M2a: ADR 0029; M2b: ADR 0036; M2c: ADR 0043):

| Directory        | Work package | Owns                                                                          |
| ---------------- | ------------ | ----------------------------------------------------------------------------- |
| `composer/`      | WP21         | HTML / Markdown composer, preview, attachments, address autocomplete          |
| `rules/`         | WP22         | rule list and editor, test preview, run history and undo                      |
| `mcp/`           | WP23         | MCP client settings, audit log, UI-context indicator and reporting            |
| `customization/` | WP24         | inspector, layout write-back, merge dialog, config errors, saved searches     |
| `security/`      | M2a WP34     | key manager, crypto settings, composer toggles, badges, warnings, vault       |
| `remote/`        | M2b WP43     | login, pairing, sessions / devices, remote-UI settings, remote content, audit |
| `launch/`        | M2b WP45     | launch requests of the desktop app (`mailto:` → composer, `.eml` → import)    |
| `sieve/`         | M2c WP54     | rule placement (server / Nail), ManageSieve state, vacation reply             |
| `triage/`        | M2c WP54     | junk marking and classifier, snooze, notification settings, Web Notifications |
| `sync/`          | M2c WP54     | per-folder sync policy, bodies on demand                                      |
| `menus/`         | M2c WP54     | right-click context menus from `layout.kdl` (`resolve.ts`)                    |

## Files of a slot

- `index.ts` — what the feature contributes, as named exports (see
  `types.ts`, `Feature`):
  - `widgets`: layout widget kind → component (`WidgetProps`). Register the
    same kind with its options in `crates/nail-config/src/widgets/<feature>.rs`.
  - `actions`: action id → `ActionHandler` (toolbars, menus and key bindings
    refer to the id). Use a feature prefix (`rules.run`, `mcp.revoke`, …).
  - `dialogs`: name → component (`DialogProps`: `name`, `close()`). A dialog
    opens with `window.open "<name>"` (or `store.openWindow(name)`) when the
    layout has no `window "<name>"`, inside the standard modal window frame;
    its title is the Fluent message `window-<name>`. Use dialogs only for
    small fixed-purpose windows; anything users should arrange is a layout
    window made of widgets.
  - `events`: handlers called with every core event (refetch queries, update
    feature state).
  - `effects`: components mounted once in the app shell that render nothing
    (background work such as reporting the UI context).
- `mock.ts` — the `MockTransport` answers for this feature's requests (the
  list `<FEATURE>_REQUESTS`; `src/api/mock.ts` routes them here) and the
  feature's mock state (`data.features.<feature>`).
- `messages.ts` — Fluent message prefixes and runtime-built ids of the
  feature, read by the catalog coverage test.
- Components, hooks and tests of the feature live in the same directory.
  Keep feature UI state in the feature (a Zustand store of its own) rather
  than in `src/state/uiStore.ts`.

## Wiring (already done, do not edit)

`registry.ts` merges the built-in widgets and actions with the slots
(`appRegistry`) and throws on a duplicate widget kind, action id or dialog
name. `App.tsx` and the test harness use `appRegistry` for the widget and
dialog registries, the action handlers, the event handlers and the effects.

## Strings

Each feature has its own catalogs: `locales/en/<feature>.ftl` (complete) and
`locales/de/<feature>.ftl` (same file, empty). The i18n loader bundles every
`.ftl` file in `locales/<lang>/` automatically. Put message ids with the
prefixes from `messages.ts` into the feature's `.ftl`, and list ids built at
runtime in `runtimeIds()` so the coverage test checks them.

## Rules that still apply

No literal user-visible strings, no inline styles or class names (only
`data-part` attributes, styled from `theme.kdl`), structure from
`layout.kdl`, data only through `NailClient`, every write through the core.
Shared files (`App.tsx`, `src/api/mock.ts`, `src/state/uiStore.ts`,
`defaults/*.kdl`) were integrator-owned while the M1b packages were built
in parallel; WP25 wired the features into them (ADR 0027). Parallel work
in later waves follows the same rule.
