# Theme reference (`theme.kdl`)

Everything Nail looks like — colours, fonts, spacing, icons, how mail is
displayed — comes from one file, `theme.kdl`, in your Nail configuration
folder. The layout (which panes exist and where) lives in
[`layout.kdl`](layout-reference.md).

Most people never need to edit it: **Settings → Appearance** picks one of
the five built-in themes, Follow Windows, the density, the text size, the
corner radius and the accent colour, and writes them into this file for you
(see [Appearance](appearance.md)). This page is the reference for writing
the file, or your own theme files, by hand.

## The short version

Create `theme.kdl` in your configuration folder and change only what you
want. Everything else comes from the built-in default theme:

```kdl
theme "My theme" extends="default" {
    tokens {
        color "accent" "#88c0d0"
        space "row" 4
    }
    part "message-list.row:unread .subject" { font-weight 700; }
}
```

Save the file and Nail updates immediately. If the file has a mistake, Nail
keeps showing the last working theme and tells you the line and column of
the problem.

The complete default theme ships with Nail as `defaults/theme.kdl`; it is a
good place to look up names and copy from.

## File structure

The file contains one `theme` node:

```kdl
theme "<name>" extends="default" mode="system" {
    appearance follow-system=#true light="frost" dark="graphite" density="comfortable"
    tokens { … }
    modes { … }
    part "<selector>" { … }
    icon "<name>" svg="…"
    message { … }
    sounds { … }
    raw-css "<part>" "<declarations>"
}
```

| Property | Meaning |
| --- | --- |
| `"<name>"` | The theme's name (required). |
| `extends` | What the file starts from: `"default"` (the base theme in `defaults/theme.kdl`), a built-in theme id (`frost`, `sage`, `graphite`, `nocturne`, `high-contrast`) or one of your themes in `themes/` (see [Appearance](appearance.md#your-own-themes)). In your `theme.kdl` it is the theme used while Follow Windows is off. Without `extends`, the file must define everything itself (at least the complete `message` section) and is used as it is. |
| `mode` | `system` (follow Windows, the default), `light`, `dark` or `high-contrast`. In `theme.kdl`, leave it out: the chosen theme sets it. |

`appearance` (only in your `theme.kdl`; every property optional) holds what
Settings → Appearance chooses: `follow-system` (`#true` / `#false`), the
Follow Windows pair `light` and `dark` (theme ids) and `density`
(`comfortable` or `compact`). Without it, Nail follows Windows with Frost and
Graphite. Three tokens of the file are the Appearance page's overrides and
apply on top of every theme: `font-size "body"` (11–20), `radius "md"`
(0–20) and `color "accent"` (`#rgb` / `#rrggbb`, one value for every mode).

```kdl
theme "My settings" extends="sage" {
    appearance follow-system=#false density="compact"
    tokens {
        color "accent" "#2f7d3a"
        font-size "body" 15
    }
}
```

The file format is [KDL](https://kdl.dev) version 2: `// comments`,
`"strings"`, numbers, `#true` / `#false`, and `;` to put several nodes on one
line. A string containing quotes can be written as a raw string:
`#""Segoe UI", sans-serif"#`.

## Tokens

Tokens are named values you can reuse everywhere as `$<category>.<name>`,
for example `$color.accent` or `$space.sm`.

```kdl
tokens {
    color "bg"     "#ffffff" dark="#1b1e24" high-contrast="#000000"
    color "accent" light="#2e5eaa" dark="#7aa7e8"
    font  "ui"     "Inter, system-ui, sans-serif"
    space "row"    6
    border "thin"  "1px solid $color.border"
}
```

- The first value is used in light mode (and in any mode without its own
  value). `dark=` and `high-contrast=` set the value for those modes.
  `light=` may be used instead of the first value.
- `print=` sets the value used while Nail prints (the print view of a
  message, conversation, event or agenda, and PDF files). A mode without a
  `print=` value prints with the light value, so a dark theme never prints
  dark. The default theme prints black text on white without shadows.
- Tokens may use other tokens (`border "thin" "1px solid $color.border"`).
  A token must not refer to itself, directly or through others.
- Numbers without a unit are pixels for `font-size`, `space`, `size` and
  `radius` tokens (`space "row" 6` is `6px`).
- Write `$$` for a literal dollar sign.
- `compact=` sets the value used with the Compact density
  (`space "row" 12 compact=8`). The base theme gives Compact values for row
  and pane padding, body and title text, control and avatar sizes.
- Redefining a default token replaces it completely, including its dark,
  high-contrast and compact values. To change only one mode, use `modes`:

```kdl
modes {
    dark { color "bg" "#101010"; }
    high-contrast { color "accent" "#ff00ff"; }
    print { color "fg" "#202020"; }
}
```

Token categories:

| Category | Used for | Default names |
| --- | --- | --- |
| `color` | Colours | surfaces `bg`, `bg-subtle`, `bg-raised`, `canvas` (the window behind the panes), `rail`; text `fg`, `fg-muted`, `fg-subtle` (tertiary); lines `border` (dividers), `border-strong` (input outlines); `accent`, `accent-fg`, `accent-soft` (pressed chips, primary icon buttons), `selection`, `selection-fg`, `unread`, `flagged`, `focus-ring`, `danger`, `warning`, `success`, `quote-1` … `quote-3`, `scrim`; `avatar-fg`, `avatar-1` … `avatar-8`, `file-pdf`, `file-image`, `file-document`, `file-spreadsheet`, `file-archive`, `file-calendar`, `file-other` (avatars and file-type badges) |
| `font` | Font families | `ui` (Segoe UI Variable), `display`, `reader`, `mono` |
| `font-size` | Type scale | `label` 11.5, `small` 12.5, `body` 14 (Compact 13), `heading` 16, `title` 20 (Compact 18); the other steps are calculated from `body`, so changing it scales them all. `xs`, `sm`, `md`, `lg`, `xl` are kept as aliases of `label`, `small`, `body`, `heading`, `title` |
| `font-weight` | Weights | `regular`, `medium`, `semibold`, `bold` |
| `line-height` | Line heights | `tight` 1.25, `normal` 1.45, `relaxed` 1.65 (the reader) |
| `space` | Spacing on a 4 px grid | `xxs` 2, `xs` 4, `sm` 8, `md` 12, `lg` 16, `xl` 24, `xxl` 32; `pane` 16 (pane padding; Compact 12), `gap` 8, `section` 24, `row` 12 (list rows; Compact 8) |
| `size` | Fixed sizes | `icon` 16, `control` 32 (button and input height; Compact 28), `avatar` 34 (Compact 28), `touch-target`, `toolbar`, `divider` |
| `radius` | Corner radii | `md` 10 (buttons, cards), `sm` and `lg` calculated from `md` (inputs and menu items; windows), `pill` 999 |
| `border` | Border shorthands | `thin`, `control` (input outlines, `border-strong`) |
| `shadow` | Shadows | `raised` (floating cards), `overlay` (menus), `window` (dialogs) |
| `motion` | Durations and easings | `fast`, `normal`, `ease` |
| `opacity` | Opacity levels | `disabled`, `muted` |
| `z-index` | Stacking order | — |

You can add your own tokens in any category.

## Parts

Every element Nail draws has a *part name*, such as `message-list.row` or
`toolbar.button.icon`. A `part` rule styles it:

```kdl
part "message-list.row" {
    padding "$space.row" "$space.md"
    border-bottom "$border.thin"
}
```

Each line inside is a CSS property followed by its value. Several values are
joined with spaces (`padding "4px" "8px"` is `padding: 4px 8px`). Numbers
without a unit are pixels for lengths such as `padding`, `margin`, `width`,
`gap` or `font-size`, and plain numbers otherwise (`font-weight 700`,
`flex-grow 1`).

When your file uses the same selector as a default rule, your properties are
added to that rule and override the default values of the same properties.

### Selectors

| Selector | Matches |
| --- | --- |
| `"message-list.row"` | every message row |
| `"message-list.row:hover"` | a row under the mouse pointer |
| `"message-list.row:selected:unread"` | a row that is selected **and** unread |
| `"folder-tree.item[role=\"inbox\"]"` | the Inbox folder |
| `"message-list.row[encrypted]"` | rows of encrypted messages |
| `"message-list.row:unread .subject"` | the subject inside an unread row (`.subject` continues the name before it: `message-list.row.subject`) |
| `"app[account=\"work\"] message-list.row"` | a row anywhere inside an element with that condition |

States: `hover`, `active`, `focus`, `focus-visible`, `focus-within`,
`selected`, `unread`, `flagged`, `disabled`, `dragover`, `expanded`.

### Part names

| Area | Parts |
| --- | --- |
| Frame | `app`, `split`, `split.pane`, `split.divider`, `stack`, `tabs`, `tabs.tab-list`, `tabs.tab`, `tabs.panel`, `text`, `spacer` |
| Toolbar | `toolbar`, `toolbar.button`, `toolbar.button.icon`, `toolbar.button.label`, `toolbar.spacer`, `toolbar.separator` |
| Status bar | `status-bar`, `status-bar.item` (condition `item`; states such as `error`, `offline`, `auth-error`, `scheduled`, `failed`), `status-bar.text`, `status-bar.button` |
| Accounts | `account-switcher`, `account-switcher.item` (condition `status`), `account-switcher.item.name`, `account-switcher.item.status` (states `online`, `connecting`, `offline`, `auth-error`, `disabled`) |
| Search | `search-bar` (state `active`), `search-bar.field`, `search-bar.icon`, `search-bar.input` (state `invalid`), `search-bar.clear`, `search-bar.error`, `search-bar.error.message`, `search-bar.error.position`, `search-bar.error.query`, `search-bar.error.before`, `search-bar.error.mark`, `search-bar.error.after`, `search-bar.hints`, `search-bar.hint`, `search-bar.hint.button`, `search-bar.hint.syntax`, `search-bar.hint.description`, `message-list.search-summary` |
| Conversation | `thread-view`, `thread-view.header`, `thread-view.subject`, `thread-view.count`, `thread-view.list`, `thread-view.item` (states `selected`, `unread`, `flagged`, `expanded`), `thread-view.item.header`, `thread-view.item.toggle`, `thread-view.item.from`, `thread-view.item.date`, `thread-view.item.preview`, `thread-view.item.open`, `thread-view.text`, `thread-view.body`, `thread-view.quote-block`, `thread-view.quote-toggle`, `thread-view.quote` |
| Windows | `window-layer` (attribute `modal` while the main window is blocked), `window-scrim` (over the main window behind a question), `window` (condition `window`: the window's id, e.g. `rules` or `composer:w1`; states `active`, `maximized`, `snapped`; attributes `snap` = `left` / `right`, `modal`, `owner` (what a question blocks: `main` or a window id), `blocked` (its question is open), `mode` = `move` / `size` (`Alt+F7` / `Alt+F8`), `dragging` = `move` / `resize`), `window.header` (the title bar), `window.title`, `window.maximize`, `window.maximize.icon`, `window.pop-out`, `window.pop-out.icon` (desktop app; state `error`), `window.close`, `window.close.icon`, `window.hint`, `window.status` (for screen readers only), `window.handle` (attribute `edge` = `n` `e` `s` `w` `ne` `se` `sw` `nw`), `window-snap-preview` (attribute `snap`), `window.body`. Position, size and stacking come from the window manager, not from the theme: set default and smallest sizes in `layout.kdl` ([Windows](windows.md)). Pop-out windows of the desktop app: `pop-out` (attribute `window`), `pop-out.header`, `pop-out.title`, `pop-out.error`, `pop-out.return`, `pop-out.return.icon`, `pop-out.return.label`, `pop-out.body` |
| Forms | `form.section`, `form.section.title`, `form.field` (condition `field`), `form.label`, `form.input`, `form.select`, `form.textarea`, `form.hint`, `form.check`, `form.check.label`, `form.checkbox`, `form.button` (conditions `action` and `variant`: `primary`, `danger`), `form.button.icon`, `form.button.label` |
| Composer | `composer`, `composer.field` (condition `field`: `identity`, `to`, `cc`, `bcc`, `subject`), `composer.field.label`, `composer.field.input`, `composer.identity`, `composer.extra-fields`, `composer.body.label`, `composer.body`, `composer.error`, `composer.send-later`, `composer.send-at`, `composer.footer` (sticks to the bottom: reminder, error, send time and actions), `composer.actions`, `composer.spacer`, `composer.saved`, `composer.empty` |
| Rich composer | `composer.toolbar`, `composer.modes`, `composer.mode` (condition `mode`; state `selected`), `composer.option`, `composer.option.checkbox`, `composer.option.label`, `composer.address`, `composer.suggestions`, `composer.suggestion` (state `active`), `composer.suggestion.name`, `composer.suggestion.email`, `composer.format`, `composer.format.label`, `composer.format.button` (condition `command`; state `active`), `composer.link`, `composer.link.input`, `composer.html` (state `source`), `composer.html.editor`, `composer.html.content`, `composer.html.source`, `composer.markdown` (state `split`), `composer.markdown.source`, `composer.markdown.editor`, `composer.markdown.content`, `composer.preview` (state `error`), `composer.preview.frame`, `composer.preview.error`, `composer.lossy`, `composer.lossy.message`, `composer.reminder` (state `confirm`), `composer.reminder.message`, `composer.attachments`, `composer.attachments.list`, `composer.attachment` (state `inline`), `composer.attachment.name`, `composer.attachment.size`, `composer.attachment.inline`, `composer.file-input`, `composer.uploading`, `composer.saved` (states `saving`, `saved`, `failed`) |
| Filter rules | `rules`, `rules.list`, `rules.list.items`, `rules.list.item` (conditions `selected`, `enabled`), `rules.list.select`, `rules.list.triggers`, `rules.editor`, `rules.views`, `rules.view`, `rules.conditions`, `rules.group` (conditions `mode`: `all`, `any`, `not`; `depth`), `rules.group.title`, `rules.group.items`, `rules.group.item`, `rules.condition` (condition `kind`), `rules.add`, `rules.actions`, `rules.actions.items`, `rules.action` (condition `kind`), `rules.text`, `rules.text.source`, `rules.text.errors`, `rules.text.error`, `rules.problems`, `rules.problems.title`, `rules.problems.items`, `rules.problem`, `rules.notice` (condition `kind`: `error`, `ok`), `rules.buttons`, `rules.hint`, `rules.test`, `rules.test.summary`, `rules.test.matches`, `rules.test.match`, `rules.test.subject`, `rules.test.from`, `rules.test.actions`, `rules.test.action`, `rules.test.why`, `rules.test.reason`, `rules.runs`, `rules.runs.title`, `rules.runs.items`, `rules.run` (states `running`, `done`, `failed`, `undone`), `rules.run.summary` |
| MCP clients | `mcp-clients`, `mcp-clients.intro`, `mcp-clients.server` (states `running`, `stopped`), `mcp-clients.create`, `mcp-clients.create.error`, `mcp-clients.list`, `mcp-clients.list.item`, `mcp-clients.client` (state `selected`; condition `access`: `granted`, `no-access`, `revoked`), `mcp-clients.client.name`, `mcp-clients.client.status`, `mcp-clients.client.hint`, `mcp-clients.client.used`, `mcp-clients.detail`, `mcp-clients.revoke`, `mcp-clients.confirm`, `mcp-clients.confirm.text`, `mcp-clients.error`, `mcp-clients.empty`, `mcp-clients.activity`, `mcp-token`, `mcp-token.title`, `mcp-token.warning`, `mcp-token.value`, `mcp-token.actions`, `mcp-token.usage`, `mcp-token.next`, `mcp-token.copy-failed`, `mcp-grants` (states `empty`, `granted`, `revoked`), `mcp-grants.fields`, `mcp-grants.empty`, `mcp-grants.scope`, `mcp-grants.scope.hint`, `mcp-grants.folders.account`, `mcp-grants.folders.summary`, `mcp-grants.folders.list`, `mcp-grants.actions`, `mcp-grants.message` (states `ok`, `error`), `mcp-audit`, `mcp-audit.toolbar`, `mcp-audit.table`, `mcp-audit.head`, `mcp-audit.body`, `mcp-audit.row` (conditions `outcome`, `kind`), `mcp-audit.time`, `mcp-audit.client`, `mcp-audit.call`, `mcp-audit.kind`, `mcp-audit.name`, `mcp-audit.outcome` (condition `outcome`: `allowed`, `denied`, `declined`, `error`), `mcp-audit.detail`, `mcp-audit.error`, `mcp-audit.empty`, `mcp-context-indicator` (states `idle`, `reading`, `editing`), `mcp-context-indicator.dot`, `mcp-context-indicator.text`, `mcp-context-indicator.dismiss` |
| Customization | `inspector` (states `picking`, `pinned`), `inspector.highlight`, `inspector.header`, `inspector.title`, `inspector.hint`, `inspector.button`, `inspector.section`, `inspector.heading`, `inspector.path`, `inspector.path.item` (state `current`), `inspector.states`, `inspector.state`, `inspector.condition`, `inspector.rules`, `inspector.rule`, `inspector.rule.selector`, `inspector.rule.when`, `inspector.rule.declarations`, `inspector.rule.declaration`, `inspector.node`, `inspector.node.kind`, `inspector.node.place`, `inspector.none`, `merge-dialog` and its children (`merge-dialog.section`, `merge-dialog.item`, `merge-dialog.diff`, `merge-dialog.side` (states `mine`, `theirs`), `merge-dialog.side.text`, `merge-dialog.conflict` (condition `kind`), …; the inspector names every one), `config-errors` (states `valid`, `invalid`), `config-errors.summary`, `config-errors.list`, `config-errors.file`, `config-errors.file.name`, `config-errors.item`, `config-errors.item.message`, `config-errors.note`, `smart-folders`, `smart-folders.list`, `smart-folders.entry`, `smart-folders.item` (state `selected`), `smart-folders.item.name`, `smart-folders.remove`, `smart-folders.add`, `smart-folders.form`, `smart-folders.name`, `smart-folders.empty`, `smart-folders.error` |
| Outbox | `outbox`, `outbox.list`, `outbox.item` (states `scheduled`, `sending`, `failed`, `draft`), `outbox.item.subject`, `outbox.item.recipients`, `outbox.item.status`, `outbox.item.state`, `outbox.item.time`, `outbox.item.error`, `outbox.item.progress` (upload progress of a large message), `outbox.item.actions`, `outbox.empty`, `outbox.error`, `outbox.drafts`, `outbox.drafts.title` |
| Move to folder | `folder-picker`, `folder-picker.filter`, `folder-picker.input`, `folder-picker.list`, `folder-picker.item` (condition `depth`), `folder-picker.button` (condition `role`), `folder-picker.button.icon`, `folder-picker.button.name`, `folder-picker.empty` |
| Account setup | `account-setup` (condition `step`), `account-setup.steps`, `account-setup.step`, `account-setup.intro`, `account-setup.form`, `account-setup.candidates`, `account-setup.candidate`, `account-setup.candidate.source`, `account-setup.candidate.servers`, `account-setup.oauth` (states `waiting`, `done`), `account-setup.oauth.status`, `account-setup.oauth.link`, `account-setup.results`, `account-setup.result` (states `ok`, `error`), `account-setup.result.title`, `account-setup.result.status`, `account-setup.result.capabilities`, `account-setup.notice`, `account-setup.actions`, `account-setup.error` |
| Settings | `account-settings`, `account-settings.sidebar`, `account-settings.list`, `account-settings.account` (condition `status`), `account-settings.account.name`, `account-settings.account.email`, `account-settings.account.status`, `account-settings.form`, `account-settings.identities`, `account-settings.identity`, `account-settings.problem`, `account-settings.message`, `account-settings.actions`, `account-settings.confirm`; `virtual-editor` with the same sub-parts plus `virtual-editor.mappings`, `virtual-editor.mapping`, `virtual-editor.sources`, `virtual-editor.source`, `virtual-editor.rules`, `virtual-editor.rule` |
| Folders | `folder-tree`, `folder-tree.item` (condition `role`: `inbox`, `sent`, `drafts`, `archive`, `junk`, `trash`, `other`), `folder-tree.item.icon`, `folder-tree.item.name`, `folder-tree.item.unread-count` |
| Message list | `message-list`, `message-list.row` (states `selected`, `unread`, `flagged`), `message-list.row.line`, `message-list.row.from`, `message-list.row.subject`, `message-list.row.date`, `message-list.row.preview`, `message-list.empty` |
| Message view | `message-view`, `message-view.header`, `message-view.header.subject`, `message-view.header.field`, `message-view.remote-content-banner`, `message-view.body`, `message-view.attachments`, `message-view.attachment`, `message-view.empty` |
| Command palette | `palette`, `palette.search`, `palette.input`, `palette.status`, `palette.list`, `palette.item` (condition `state`: `active`, `unavailable`, `recent`; condition `source`), `palette.item.title`, `palette.item.badge`, `palette.item.unavailable`, `palette.shortcuts`, `palette.shortcut`, `palette.match`, `palette.empty`, `palette.footer`, `palette.hint` |
| Printing | `print-setup`, `print-setup.what`, `print-setup.nothing`, `print-setup.saved`, `print`, `print.toolbar`, `print.page`, `print.message`, `print.subject`, `print.headers`, `print.header`, `print.header.name`, `print.header.value`, `print.body`, `print.text`, `print.quote`, `print.quote-omitted`, `print.attachments`, `print.attachments.list`, `print.attachment`, `print.thread`, `print.thread.title`, `print.thread.count`, `print.event`, `print.event.title`, `print.attendees`, `print.attendees.list`, `print.attendee`, `print.section.title`, `print.agenda` (with `.heading`, `.group`, `.day`, `.date`, `.events`, `.event`, `.time`, `.title`, `.location`, `.free`, `.month`, `.caption`, `.week-row`, `.cell`, `.cell.day`), `print.error`, `print.recovery-kit`; colours come from the `print` mode |
| Import, export, backups | `wizard`, `wizard.header`, `wizard.step`, `wizard.title`, `wizard.intro`, `wizard.step-body`, `wizard.buttons`, `path-field`, `form.radio`, `form.error`, `import-export`, `backup-restore`, `portability.choices`, `portability.choice`, `portability.found` (`.item`, `.label`, `.default`, `.used`), `portability.issues` (`.count`, `.list`), `portability.issue` (conditions `severity`: `warning`, `skipped`; `code`) with `.severity`, `.text`, `.where`, `.path`, `.index`, `.label`, `.details`, `.detail`, `portability.report` (conditions `state`, `kind`) with `.heading`, `.title`, `.path`, `.when`, `portability.progress` (`.bar`, `.phase`, `.items`, `.bytes`, `.current`), `portability.jobs`, `portability.job` (condition `state`) with `.kind`, `.path`, `.state`, `.when`, `.progress`, `portability.manifest`, `portability.plan` (`.caption`, `.head`, `.row`, `.item`, `.effect`, `.numbers`, `.note`), `portability.proposals`, `portability.proposal` (condition `state`: `incomplete`, `added`), `portability.verify` |
| Diagnostics | `diagnostics-settings`, `diagnostics` (condition `state`: `options`, `previewed`, `saved`), `diagnostics.issues`, `diagnostics.preview`, `diagnostics.preview.summary`, `diagnostics.entries`, `diagnostics.entry` with `.summary`, `.name`, `.item`, `.size`, `.redactions`, `.redaction`, `.details`, `.text`, `diagnostics.saved`, `diagnostics.saved.text`, `diagnostics.saved.hash` |
| Recovery key, smartcards | `recovery`, `recovery.text`, `recovery.result`, `recovery-key`, `recovery-key.value`, `recovery-key.group`, `recovery-settings` with `.facts`, `.fact`, `.fact.label`, `.fact.value`, `.confirm`, `smartcards` with `.modules`, `.module`, `.module.path`, `.tokens`, `.token`, `.token.label`, `.token.facts`, `.objects`, `.object` and `.object.kind`, `.label`, `.algorithm`, `.usages`, `.certificate`, `.fingerprint`, `.used`, `.actions`, `pin-prompt` with `.purpose`, `.token`, `.key`, `.text` |

### Supported CSS properties

Layout: `display`, `position`, `top`, `right`, `bottom`, `left`, `inset`,
`z-index`, `overflow`, `overflow-x`, `overflow-y`, `box-sizing`,
`visibility`, `width`, `height`, `min-width`, `min-height`, `max-width`,
`max-height`, `aspect-ratio`, `margin` (and `-top`, `-right`, `-bottom`,
`-left`, `-inline`, `-block`), `padding` (same variants).

Flex and grid: `flex`, `flex-direction`, `flex-wrap`, `flex-grow`,
`flex-shrink`, `flex-basis`, `order`, `align-items`, `align-self`,
`align-content`, `justify-content`, `justify-items`, `justify-self`, `gap`,
`row-gap`, `column-gap`, `grid-template-columns`, `grid-template-rows`,
`grid-column`, `grid-row`.

Text: `color`, `font`, `font-family`, `font-size`, `font-weight`,
`font-style`, `font-variant-numeric`, `font-feature-settings`,
`line-height`, `letter-spacing`, `text-align`, `text-decoration`,
`text-decoration-color`, `text-transform`, `text-overflow`, `text-wrap`,
`white-space`, `word-break`, `overflow-wrap`, `vertical-align`.

Backgrounds and borders: `background`, `background-color`, `border`,
`border-top`, `border-right`, `border-bottom`, `border-left`,
`border-color`, `border-width`, `border-style`, the per-side `-color` and
`-width` variants, `border-radius` and its four corners, `border-collapse`, `outline`,
`outline-color`, `outline-width`, `outline-style`, `outline-offset`,
`box-shadow`, `opacity`, `accent-color`, `caret-color`, `fill`, `stroke`,
`stroke-width`.

Motion and interaction: `transition`, `transition-property`,
`transition-duration`, `transition-timing-function`, `transition-delay`,
`cursor`, `user-select`, `pointer-events`, `scrollbar-width`,
`scrollbar-color`.

For safety, values cannot load anything from outside: `url(…)`,
`image-set(…)` and `@import` are rejected, as are the characters
`; { } < > \`.

## Icons

```kdl
icon "archive" svg="M3 3h18v5H3z M4 9h16v12H4z"
icon "dot" svg="M0 0h2v2H0z" view-box="0 0 2 2"
```

`svg` is SVG path data (the `d` attribute of a `<path>`), drawn in a
`0 0 24 24` box unless `view-box` says otherwise. Several icons can be
grouped in `icons { … }`.

Nail draws its icons from the [Lucide](https://lucide.dev) set (names such
as `reply`, `archive`, `trash`, `square-pen`, `refresh-cw`); toolbars and
menus in `layout.kdl` name them. An `icon` in your theme with the same name
replaces the Lucide icon everywhere, and a new name adds an icon you can use
in `layout.kdl`. Older Nail icon names (`compose`, `delete`, `sync`, …) still
work and show the matching Lucide icon.

## Message display

```kdl
message {
    font-family "$font.reader"
    text-font-family "$font.mono"
    font-size "$font-size.md"
    line-height "$line-height.normal"
    max-width "80ch"
    quote-colors "$color.quote-1" "$color.quote-2" "$color.quote-3"
    html-dark-mode "smart"
}
```

| Setting | Meaning |
| --- | --- |
| `font-family` | Font of the reading pane |
| `text-font-family` | Font of plain-text mail |
| `font-size`, `line-height` | Text size and spacing (a plain number for `font-size` is pixels) |
| `max-width` | Maximum width of the text column, e.g. `"72ch"` or `"none"` |
| `quote-colors` | Colours of quote levels 1, 2, 3, … (repeated for deeper levels) |
| `html-dark-mode` | How HTML mail looks in dark mode: `off` (as sent), `invert`, `smart` (recolour, keep hues) |

## Sounds

```kdl
sounds {
    mail.new "chime"
    calendar.reminder "sounds/bell.ogg"
}
```

Each line is an event and a built-in sound name or a file inside your
configuration folder. The default theme is silent.

## Raw CSS (unstable)

```kdl
raw-css "message-list.row" "text-wrap: balance;"
```

For properties the theme does not support yet. The CSS applies to the named
part only, tokens work as usual, and the same safety rules apply (no
`url()`, no `@import`, no braces). Raw CSS may stop working in a later
version when part structure changes.

## High contrast and reduced motion

High contrast is a built-in theme: pick it in Settings → Appearance, or let
Follow Windows switch to it while Windows uses a contrast theme. It has 7:1
contrast for everything, bigger text, 2 px borders and thick focus rings
(`defaults/themes/high-contrast.kdl`; see
[Accessibility](accessibility.md#high-contrast)). The base theme also has
`high-contrast=` values for its colours, used by your own themes with
`mode="high-contrast"`.

When Windows asks for fewer animations, Nail stops all transitions and
animations, whatever the theme says. Set the `motion` tokens to `0ms` to
turn them off always.

New parts of this version: `context-menu`, `context-menu.item` (states
`focus`, `disabled`, `expanded`), `context-menu.item.icon`,
`context-menu.item.label`, `context-menu.item.shortcut`,
`context-menu.item.arrow`, `context-menu.separator`, `context-menu.popup`;
`message-list.header`, `message-list.row.columns`, `message-list.row.cell`
(condition `column`), the row state `headers-only`; `junk-verdict`,
`sync-message-body` (state `failed`, `fetching`), `snooze`,
`snooze.preset`, `snooze-list`, `snooze-list.item`, `sieve-status`,
`vacation-editor`, `junk-settings`, `notification-settings`,
`sync-folder-policies`, `sync-policy`, `toolbar-editor`,
`key-binding-editor`, `column-editor` with their sub-parts (use the
inspector to find them), and `composer.editor-loading`.

## Error messages

A mistake is reported with the file, line and column, for example:

<!-- A misspelled token on purpose. spellchecker:off -->
```text
theme.kdl:12:17: unknown token `$color.acent`
```
<!-- spellchecker:on -->

Until you fix it, Nail keeps using the last theme that worked.
