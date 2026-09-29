# ADR 0008: React renderer architecture (M0 WP4)

Status: accepted · 2026-09-29 · M0 WP4

## Context

The renderer (`ui/`) must build the whole UI from the resolved layout and
theme (ADR 0003), run unchanged over Tauri IPC and a WebSocket (ADR 0002),
and never use inline styles, literal class names or literal UI text.
Some things the design leaves open had to be decided.

## Decisions

### Libraries (all already in `ui/package.json`, latest stable)

| Concern | Choice | Why |
| --- | --- | --- |
| Server data | TanStack Query 5 | Caching, invalidation on `MailChanged`, infinite pages for the list |
| UI state | Zustand 5, one vanilla store per app instance (React context) | Tests and future multi-window get isolated stores |
| Virtual list | TanStack Virtual 3 | Dynamic row measurement; headless |
| i18n | `@fluent/bundle` + `@fluent/langneg` | Same format as the core; catalogs imported raw via `import.meta.glob("@locales/*/*.ftl")` |

No other runtime dependencies were added.

### Client and transports

`NailClient` (`src/api/client.ts`) wraps a `Transport` (`send`, `onEvent`,
`onStatus`). `request(req, "Accounts")` narrows the response type and turns
`Error` responses into `NailError { code }`; widgets show
`error-<code>` from Fluent. Transport choice: `?mock=1` → `MockTransport`
(fixture theme/layout + generated mailbox), `window.__TAURI_INTERNALS__` →
`TauriTransport`, else `WebSocketTransport`. Transports are lazy chunks.

WebSocket: requests made while disconnected are queued and sent after
connect; requests in flight when the socket drops fail with `network`
(they may or may not have run — the UI refetches instead of replaying);
reconnect backoff doubles from 500 ms to a 10 s cap; after a reconnect all
queries are invalidated.

### One stylesheet, plus geometry rules

The `ThemeProvider` owns the only `<style data-nail-theme>` element.
Content: structural base rules → tokens on `:root` → per-mode overrides on
`:root[data-nail-mode="…"]` → reader rules → part rules → raw CSS. The
provider sets `data-nail-mode` on `<html>` to the *effective* mode (theme
mode or user override; for `system` from `prefers-color-scheme` /
`prefers-contrast`), so a mode switch never recompiles the sheet.

Sizes that come from data rather than the theme — split pane sizes (layout +
dragging) and the virtual list's spacer heights — cannot be inline styles.
They are **geometry rules** `[data-geom="<id>"] { … }` inserted via CSSOM
into the same sheet by `StyleManager` (`useGeometry(id, decls)`), updated in
place, and re-inserted after a theme swap. Everything written to the sheet
passes a value check (no `{ } ; < > \`, comments, `url(`, `@import`,
`expression(`); unsafe theme rules are dropped with a warning.

Split panes use flexbox (`240px` → `flex: 0 0 240px`, `1.4fr` →
`flex: 1.4 1 0px`); stacks use CSS grid rows. Dragging a divider converts a
fixed pane to pixels or, between two `fr` panes, redistributes their
weights so the split stays fluid. Dragged sizes live in the UI store (not
yet written back to `layout.kdl`; that is M1).

### Reader iframe

`sanitized_html` renders in `<iframe sandbox="" srcdoc>` (no scripts, opaque
origin). The document carries its own CSP (`default-src 'none';
style-src 'unsafe-inline'; img-src data: cid:; font-src data:`) and a
reader stylesheet generated from the theme's `message` section with token
references resolved to literal values for the active mode, because the
iframe cannot see the app's custom properties. Text mail renders in
`<pre data-part="message-view.text">`.

### Template language

Hand-written parser, no `eval`: `{path | pipe | pipe(arg)}`; pipes
`relative`, `short-date`, `truncate(n)`, `bold-if-unread`, `chips`.
Unknown pipes and wrong arguments are compile errors; field lookup uses own
properties only. An address's `name` falls back to its email. Field
`from.name` renders as part `message-list.row.from`. Invalid rows are
dropped with a warning; if none remain the default rows are used.
`bold-if-unread` sets `data-emphasis="strong"` (a base rule maps it to
`--nail-font-weight-bold`).

### Actions and keys

Built-in handlers: `mail.next`, `mail.previous`, `view.toggle <node-id>`,
`sync.now`. Other actions in the default layout (reply, archive, flags…)
need write requests that arrive in M1; they render as disabled buttons and
their keys do nothing. Layout aliases (`alias_of`) are followed up to 8
levels. The keyboard context is the closest `data-key-context` of the
focused element (`list`, `reader`, `folders`), else the last one focused;
bindings of that context win over `global`, later bindings win within a
context. Sequences wait 1 s. Keys are ignored in editable fields.

### Parts added beyond ADR 0003

`app.status`, `layout.unknown-widget`, `layout.missing`,
`<widget>.error`, `message-list.loading`, `message-list.spacer`,
`message-list.row.text`, `message-list.row.chip`, `message-view.loading`,
`message-view.text`, `message-view.header.label`,
`message-view.header.value`, `message-view.attachment.name`,
`message-view.attachment.size`, `folder-tree.list`. Extra row states:
`answered`, `has-attachments`. Documented for users in
`userDocs/en/templates-and-parts.md`.

### URL overrides

`?lang=<tag>` overrides the locale, `?mode=<mode>` the colour mode (e2e and
remote UI); the core's `Config.locale` applies when no override is given.

## Consequences

- Widgets are pure functions of their layout node, the UI store and queries;
  each is unit-tested against `MockTransport`.
- `ui/src/source-rules.test.ts` fails if any component source contains
  `style=`, a literal `className` or `dangerouslySetInnerHTML`, in addition
  to the ESLint rules.
- Because the fixtures omit empty fields, `src/api/normalize.ts` fills
  defaults at the boundary; the core's full JSON passes through unchanged.
