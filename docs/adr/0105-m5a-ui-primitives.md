# ADR 0105: WP153 — UI primitives on the M5a tokens, Lucide icons, mail-pane fixes, appearance page

Status: accepted · 2026-10-03 · M5a WP153

## Context

Plan section 10, wave M5a, WP153: restyle the UI primitives on the token
schema of ADR 0102, use Lucide icons everywhere, fix the alignment (one 4 px
grid, pane padding 16), replace the broken attachment chips, put the reader
body in the UI face in a centred column of about 68 characters, give the
message list rows avatar / unread dot / tag line (hidden in Compact), switch
themes on `ThemeChanged` and report Follow Windows changes from the
browser / WebView. The look is the approved proposal's
(a published page, not linked here).

WP153 owns `ui/` and `locales/` only. The shipped part rules live in
`defaults/theme.kdl` (WP151's slot in this wave), so the visual defaults of
the primitives cannot be written there by WP153.

## Decisions

### The skin: a token-only default layer in the compiler

`ui/src/theme/skin.ts` (`SKIN_CSS`) holds the M5a look of the shared
primitives and the mail panes. `compileTheme` emits it right after the
structural `BASE_CSS` and **before** the token rules and the theme's part
rules, so:

- every theme rule (`theme.kdl`, a built-in theme, the user's file) still
  wins over it — it is a default, not a lock, and stays expressible in
  `theme.kdl`;
- it uses tokens only (`var(--nail-…)`). Literal colours appear only as
  fallbacks of tokens the schema does not have yet (`--nail-color-avatar-1…8`,
  `--nail-color-avatar-fg`, `--nail-color-file-*`, `accent-soft`,
  `border-strong`, `font.display`); `skin.test.tsx` enforces this and checks
  white text on every avatar / file-type fallback colour for WCAG AA.

Primitives and their parts / variants:

| Primitive | Parts (variants) |
| --- | --- |
| Buttons | `form.button` with `data-variant` = (none) default, `primary`, `danger`, `ghost`, `icon`; `toolbar.button` (ghost); `window.close`; `form.button.icon` |
| Inputs, selects | `form.input`, `form.select`, `form.textarea`, `search-bar.field`; labels `form.label`, hints `form.hint`, errors `form.error` |
| Checks, switches | `form.check`, `form.checkbox`; `switch` + `switch.thumb` (`role="switch"`, component `Switch` in `widgets/primitives.tsx`) |
| Chips, tags, badges | `chip` / `message-list.row.chip` (`aria-pressed`), `tag` (`.dot`, `.icon`, `.text`; `data-tone` warning / danger / accent), `badge` |
| Menus | `context-menu.list`, `.item` (icon / label / shortcut / arrow columns), `.separator`, `.header` (styled; the menu model has no header entry yet — see requests) |
| Tabs, segmented | `tabs.tab-list`, `tabs.tab` (underline); `segmented` + `segmented.option` (radio group, component `Segmented`) |
| Cards, windows | `card`, `card.title`, `card.row`, `form.section` (a card); `window-layer`, `window`, `window.header`, `window.body` |

Spacing is on the 4 px grid (`space.*`), panes share `space.pane`
(toolbars, list rows, search bar, tab lists, window bodies, status bar).

### Lucide icons

`ui/src/widgets/lucideIcons.ts` statically imports exactly
`nail_api::icons::BUNDLED_ICONS` from `lucide-react` (ISC) and mirrors
`LEGACY_ICON_NAMES`; `icons.test.tsx` keeps both equal to
`crates/nail-api/fixtures/icons.json`. `Icon` resolves the theme's own icon
→ the Lucide icon (bundled or legacy name) → nothing, and renders the
Lucide component with `data-part`, `data-icon` (the Lucide name) and
`data-icon-set="lucide"`; size comes from the part's CSS (`size.icon`).
UI code now names Lucide icons (`x`, `paperclip`, `send`, `trash`; the
composer's format buttons via `FORMAT_ICONS`).

### Mail panes

- **List rows** (`MessageList`, row view): `message-list.row.unread-dot`
  (only on unread rows), `message-list.row.avatar` (initials, `data-color` =
  a stable FNV-1a slot of the address), `message-list.row.body` with the
  layout's template lines, and `message-list.row.tag-line` with `tag`s:
  account (colour dot + name when more than one account exists),
  attachments, invitation, warning, flagged, up to three labels. The column
  view (`columns=`) keeps plain rows (`data-view="columns"`). The tag line is
  hidden in Compact by `:root[data-nail-density="compact"]` and by
  `app[density="compact"]`. ADR 0102's example name `message-list.row.tags`
  is already the `{tags}` template field, so the tag line is
  `message-list.row.tag-line`.
- **Reader**: `message-view.column` wraps header, cards, body and previews
  (centred, `max-width: 68ch + 2 × space.xxl`). The text face / line length
  stay the theme's `message` section (`text-font-family`, `max-width`), which
  the compiler emits after the skin, so picking monospace still works.
  The reader iframe body has no side padding (the column provides it).
- **Attachment cards** (`widgets/AttachmentCard.tsx`, `attachment-card.*`):
  a type badge (extension or MIME subtype, `data-kind` pdf / image /
  document / spreadsheet / archive / calendar / other), name and size in
  separate parts, actions: Preview (when a plug-in renders the type; moves
  focus to the preview pane), Save, the warning mark and the plug-ins'
  attachment actions. They sit between the header and the body.

### Density

The compiler puts the `compact` token variant under
`:root[data-nail-density="compact"]` after the colour modes (not under
`data-nail-mode`); `resolveTokenRefs` (reader iframe) applies it when the
theme is compact. `ThemeProvider` sets `data-nail-density`, the app part
carries `data-density` (ADR 0102).

### Appearance feature

- `appearance-settings` widget: theme gallery (radio cards, preview swatches
  of the built-ins via geometry rules), Follow Windows switch with the light /
  dark pair, density segmented control, overrides form (text size, radius,
  accent; validated like the core, localized issues). `unsupported`
  (`m5a-not-ready`) shows the core's reason.
- `ThemeChanged` updates the cached `Appearance` and refetches the config
  when the active theme, density or overrides changed (the core also sends
  `ConfigChanged`; the refetch covers a session that only got
  `ThemeChanged`).
- `SystemAppearanceReporter` (effect): while Follow Windows is on, reports
  page-side changes of `prefers-color-scheme` / `prefers-contrast` /
  `forced-colors` — and once at start when the core does not know the OS
  state. It never pushes back a state the core got from the OS (the
  desktop's own watcher wins until the page sees a change), and stops after
  one `unsupported` / `invalid_request`.
- Mock core: `features/appearance/themes.ts` reads `defaults/themes/*.kdl`
  (Vite `?raw`) and lays the active theme over the base like the core
  (variants of replaced tokens dropped, `compact=`, overrides; tokens the
  user changed stay the user's). `GetConfig` answers the resolved theme;
  appearance changes emit `ThemeChanged` then `ConfigChanged`.

### Requests the UI needs before the contract has them

`ui/src/api/pending.ts` sends `ReportSystemAppearance {system}` and
`SaveAttachment {message_id, index}` through the `NailClient` (one cast, in
that file). `unsupported` / `invalid_request` = not available; the mock
implements both. They go to WP155 as contract requests; once added, the
types move to the generated protocol and the cast goes.

## Consequences

- The primitives look the same everywhere a part is used (435 `form.button`s,
  every form, menu, tab list) without touching the components; themes stay
  in control.
- Until WP155 removes the superseded part rules from `defaults/theme.kdl`
  (list in the WP153 report), the shipped theme's older rules win over the
  skin for those parts; new parts (avatar, tag line, cards, switch,
  segmented, theme gallery) show the skin already.
- `SummaryExtras` (`attachment_count`, `invitation`, `warning`) are read when
  the core sends them; without them the tags simply do not appear.
