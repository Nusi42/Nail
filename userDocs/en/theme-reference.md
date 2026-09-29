# Theme reference (`theme.kdl`)

Everything Nail looks like — colours, fonts, spacing, icons, how mail is
displayed — comes from one file, `theme.kdl`, in your Nail configuration
folder. The layout (which panes exist and where) lives in
[`layout.kdl`](layout-reference.md).

This page covers what works in the current version (M0).

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
| `extends="default"` | Start from the default theme and change only what the file says. Without it, the file must define everything itself (at least the complete `message` section). |
| `mode` | `system` (follow Windows, the default), `light`, `dark` or `high-contrast`. |

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
- Tokens may use other tokens (`border "thin" "1px solid $color.border"`).
  A token must not refer to itself, directly or through others.
- Numbers without a unit are pixels for `font-size`, `space`, `size` and
  `radius` tokens (`space "row" 6` is `6px`).
- Write `$$` for a literal dollar sign.
- Redefining a default token replaces it completely, including its dark and
  high-contrast values. To change only one mode, use `modes`:

```kdl
modes {
    dark { color "bg" "#101010"; }
    high-contrast { color "accent" "#ff00ff"; }
}
```

Token categories:

| Category | Used for | Default names |
| --- | --- | --- |
| `color` | Colours | `bg`, `bg-subtle`, `bg-raised`, `fg`, `fg-muted`, `border`, `accent`, `accent-fg`, `selection`, `selection-fg`, `unread`, `flagged`, `danger`, `focus-ring`, `quote-1` … `quote-3` |
| `font` | Font families | `ui`, `reader`, `mono` |
| `font-size` | Type scale | `xs`, `sm`, `md`, `lg`, `xl` |
| `font-weight` | Weights | `regular`, `medium`, `bold` |
| `line-height` | Line heights | `tight`, `normal` |
| `space` | Spacing scale | `xxs`, `xs`, `sm`, `md`, `lg`, `xl`, `row` |
| `size` | Fixed sizes | `icon`, `touch-target`, `toolbar`, `divider` |
| `radius` | Corner radii | `sm`, `md`, `lg` |
| `border` | Border shorthands | `thin` |
| `shadow` | Shadows | `raised`, `overlay` |
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
| Status bar | `status-bar`, `status-bar.item` |
| Accounts | `account-switcher`, `account-switcher.item` |
| Folders | `folder-tree`, `folder-tree.item` (condition `role`: `inbox`, `sent`, `drafts`, `archive`, `junk`, `trash`, `other`), `folder-tree.item.icon`, `folder-tree.item.name`, `folder-tree.item.unread-count` |
| Message list | `message-list`, `message-list.row` (states `selected`, `unread`, `flagged`), `message-list.row.line`, `message-list.row.from`, `message-list.row.subject`, `message-list.row.date`, `message-list.row.preview`, `message-list.empty` |
| Message view | `message-view`, `message-view.header`, `message-view.header.subject`, `message-view.header.field`, `message-view.remote-content-banner`, `message-view.body`, `message-view.attachments`, `message-view.attachment`, `message-view.empty` |

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
`-width` variants, `border-radius` and its four corners, `outline`,
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
grouped in `icons { … }`. Default icon names: `reply`, `reply-all`,
`forward`, `archive`, `delete`, `flag`, `mark-read`, `sync`, `folder`,
`inbox`, `sent`, `drafts`, `junk`, `trash`.

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

## Error messages

A mistake is reported with the file, line and column, for example:

```
theme.kdl:12:17: unknown token `$color.acent`
```

Until you fix it, Nail keeps using the last theme that worked.
