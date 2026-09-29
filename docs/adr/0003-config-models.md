# ADR 0003: Resolved theme and layout models

Status: accepted · 2026-09-29 · M0 WP0

## Context

`nail-config` (WP1) parses `theme.kdl` / `layout.kdl`; the React renderer
(WP4) turns the result into CSS and a component tree. The two are built in
parallel, so the hand-off format must be fixed first. The plan's first idea
was "opaque JSON trees in M0"; that would leave both sides guessing, so the
models are typed now.

## Decision

`nail-api::config` defines **resolved** models — the KDL after `extends` has
been merged onto the defaults, validated, and token references rewritten.
The renderer does no resolution of its own. Every field is documented in the
Rust source and in the generated TypeScript.

The reference instances are
`crates/nail-api/fixtures/default-theme.json` and `default-layout.json`
(the default three-pane UI). WP1's shipped defaults must resolve to exactly
these; WP4's tests render them. Tests in `nail-api` check that every token,
action, icon, toolbar and message id they reference exists.

### Theme (`ResolvedTheme`)

| Field | Meaning |
| --- | --- |
| `name`, `mode` | Theme name; `system` / `light` / `dark` / `high-contrast` |
| `tokens` | `category → name → CSS value` (`color`, `font`, `font-size`, `font-weight`, `line-height`, `space`, `size`, `radius`, `border`, `shadow`, `motion`, `opacity`, `z-index`) |
| `modes` | Per-mode overrides of `tokens` (`dark`, `high-contrast`, …); base tokens are the light values |
| `parts` | Ordered `PartRule {selector, states, conditions, ancestors, declarations, source}` |
| `icons` | `name → {path, view_box}` (bundled icon-set names are resolved to paths by the core) |
| `message` | Reader display: fonts, size, line height, max width, quote colours, HTML dark mode |
| `sounds`, `raw_css` | Notification sounds; unstable per-part CSS escape hatch (no `url()` / `@import`) |

**Token references.** `$color.accent` in KDL becomes
`var(--nail-color-accent)` everywhere in the resolved model, including inside
other tokens. The renderer emits base tokens and the active mode's overrides
as custom properties (`--nail-<category>-<name>`) and never re-generates the
part stylesheet on a mode switch.

**Part selectors → DOM.** Every element renders `data-part="<part name>"`.

| Theme syntax | Resolved | CSS emitted by the renderer |
| --- | --- | --- |
| `part "message-list.row"` | `selector` | `[data-part="message-list.row"]` |
| `:hover`, `:active`, `:focus`, `:focus-visible`, `:focus-within` | `states` | the pseudo-class |
| any other state (`:selected`, `:unread`, `:flagged`, `:disabled`, `:dragover`, `:expanded`) | `states` | `[data-state~="selected"]` |
| `[tag="work"]` / `[encrypted]` | `conditions` | `[data-tag~="work"]` / `[data-encrypted]` |
| `"message-list.row:unread .subject"` | `selector: message-list.row.subject`, `ancestors: [{message-list.row, [unread]}]` | descendant combinator |

Components therefore expose application states as whitespace-separated
tokens in `data-state`, and data conditions as `data-<key>` attributes whose
values are whitespace-separated lists.

**M0 part catalogue** (from the default theme): `app`; `split`,
`split.pane`, `split.divider`; `stack`; `tabs`, `tabs.tab-list`, `tabs.tab`,
`tabs.panel`; `toolbar`, `toolbar.button`, `toolbar.button.icon`,
`toolbar.button.label`, `toolbar.spacer`, `toolbar.separator`; `status-bar`,
`status-bar.item`; `account-switcher`, `account-switcher.item`;
`folder-tree`, `folder-tree.item` (condition `role`), `folder-tree.item.icon`,
`folder-tree.item.name`, `folder-tree.item.unread-count`; `message-list`,
`message-list.row` (states `selected`, `unread`, `flagged`),
`message-list.row.line`, `message-list.row.from`, `message-list.row.subject`,
`message-list.row.date`, `message-list.row.preview`, `message-list.empty`;
`message-view`, `message-view.header`, `message-view.header.subject`,
`message-view.header.field`, `message-view.remote-content-banner`,
`message-view.body`, `message-view.attachments`, `message-view.attachment`,
`message-view.empty`; `text`; `spacer`. WP4 may add parts; the generated
reference (WP7) lists them.

### Layout (`ResolvedLayout`)

| Field | Meaning |
| --- | --- |
| `name` | Layout name |
| `windows` | `window name → [Breakpoint {min_width, max_width, root}]`; first matching breakpoint wins (bounds inclusive, `null` = open) |
| `actions` | `action id → ActionDef {label, icon, alias_of, args}`; metadata for every action the layout uses |
| `keys` | `KeyBinding {context, key, action, args}` in source order (`"ctrl+shift+m"`, sequences `"g i"`) |
| `toolbars` | `name → [ToolbarItem]` (`action` / `spacer` / `separator`), referenced by a `toolbar` widget's `toolbar` option |
| `menus` | `"app"` or `"context:<part>" → [MenuItem]` (`action` / `separator` / `submenu`) |

`LayoutNode` is tagged by `type`: `split {orientation, sizes, min_sizes,
max_sizes, collapsible, resizable, children}`, `stack {sizes, children}`,
`tabs {active, children}`, `widget {kind, options, rows, source}`. All nodes
have optional `id` and `label`. KDL's `breakpoint { window … }` nesting is
inverted to `window → breakpoints`, so the renderer picks a window tree
first and a breakpoint second. `horizontal` splits place children side by side.

**Labels.** A label is literal text or `@message.id`. Fluent identifiers
cannot contain dots, so the renderer looks up `@mail.action.reply` as
`mail-action-reply` (`label_message_id()`). This keeps the design's `@a.b.c`
syntax and valid Fluent catalogs. The default layout uses message ids only.

## Alternatives considered

- **Opaque JSON** (the plan's M0 note): no shared contract between WP1 and
  WP4, errors found only at integration.
- **A CSS string instead of part rules**: loses the inspector's
  rule-to-source mapping and makes validation and merging impossible.
- **Literal token values instead of `var()`**: a mode switch would need a new
  stylesheet, and aliases between tokens would be frozen at resolve time.

## Consequences

- New containers (`grid`, `dock`, `scroll`, `overlay`) and widgets are added
  as enum variants / kinds; ts-rs regenerates the TypeScript.
- Widget options are a JSON map (kebab-case keys as in KDL) validated by
  `nail-config`; typed per-widget option structs can come later without
  changing the wire format.
