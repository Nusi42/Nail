# ADR 0145: P1 review fixes — dark-theme paper, inline images for the reader only, body style braces

Status: accepted · 2026-10-05 · P1 review (fixes ADRs 0140, 0142, 0144)

## Context

The P1 reviews reported five must-fix and four optional findings in the wave integrated by ADR 0144. Each was
confirmed in the code (and, for the CSS ones, in the committed screenshots), fixed, and covered by a regression test.

## Decisions

### Designed mail's paper is decided outside the frame (fixes ADR 0140)

ADR 0140 decided "designed" mail inside the reader frame. The `smart` dark mode's
`:root:has(> body:has(…)) { color-scheme: light }` nests `:has()`, which CSS forbids, so Chromium dropped the rule:
designed mail kept `color-scheme: dark`, and links without their own colour took the dark scheme's `LinkText`
(`#9e9eff`, about 2.1:1 on the mail's light paper in Graphite and Nocturne). The UI already tests the sanitized HTML
(`isDesignedMail`), so `readerCss(theme, mode, designed)` now emits the paper only for designed mail:
`:root { color-scheme: light }` and the paper's colours at zero specificity after the theme's. The paper takes the
theme's **print** colours (`resolveTokenRefs(…, "print")`: white paper, black text and a link colour every built-in
theme checks on white), so it stays themeable; a theme without print colours gets `#ffffff` / `#1f1f1f` / `#0b57d0`.
`ThemeProvider` keeps both stylesheets; `useReaderCss(designed)` picks one, and the remote UI's sandbox receives the
chosen one in its `css` parameter as before. No reader rule nests `:has()` (a unit test checks it).

### High contrast forces its colours on every mail again

The high-contrast theme inherits `html-dark-mode "smart"`. Since ADR 0140 that kept designed mail (nearly every HTML
mail) in its author's colours on a white page. In `high-contrast` mode (unless the theme says `off` or `invert`) the
reader now forces the theme's text and background colours on every element and its accent on links, designed or
not — the behaviour before P1 for the users who chose high contrast.

### Inline images reach only the reader (fixes ADR 0140; additive protocol field)

ADR 0140 put inline images as `data:` URLs (up to 8 MiB per message) into `MessageBody.sanitized_html`, which also
feeds reply / forward quotes (HTML and Markdown), MCP `get_message` and plug-ins. Replies then carried megabytes of
base64 in their Markdown source and outgoing HTML. Now nail-mime sanitizes twice when a body has inline images:
`sanitized_html` keeps the small `cid:` references to the known parts (`sanitize_mail_html_portable`; references to
missing parts are still removed), and the new optional `MessageBody.reader_html` / `ParsedMessage.reader_html` holds
the reader's form with the `data:` URLs. The reader renders `reader_html ?? sanitized_html`; the sandbox route does
the same; a remote session whose reader frames the sandbox origin (`render_url`) does not receive `reader_html`.
This is the one protocol change of P1: additive, optional, ignored by every consumer but the reader.

### No braces in kept CSS declarations

`<body style>` becomes a `body { … }` rule inside the mail's style sheet. `filter_declarations` checked each
declaration only by substrings and prefixes, so `color:red} .x{position:fixed…` closed the rule and smuggled in fixed
boxes and generated link text, two guarantees of ADR 0140. Declarations containing `{` or `}` are now dropped
everywhere (attributes, style blocks, the body rule), and so are declarations with an unbalanced quote (escapes are
already refused), which could swallow the end of the rule they land in.

### Smaller fixes

- The key list (Settings → Keys & encryption) lost its browser bullets: `security-keys.list` joined BASE_CSS's
  `list-style: none` lists; `p1-windows.spec.ts` now fails on any list row with a control that still shows a marker
  in a settings section, which found the same in the account, remote-access (users, devices, sessions, passkeys,
  allow-lists) and smartcard lists (markers there were mostly clipped by the pane); they joined the list too.
- Beside the preview the theme editor's token rows stack by the panel's own width (container `nail-theme-panel`),
  so long values such as `color-mix(…)` keep a full-width field.
- The theme editor keeps its live preview in sight: three columns (tabs, panel 3fr, preview 2fr) above 80rem, tabs
  above panel and preview side by side above 52rem, and one column with the preview *above* the panel below that
  (formerly 64rem, below the whole token list).
- Settings → Appearance: the "Light theme" / "Dark theme" labels do not wrap beside their selects.
- The HTML composer's editor fills the height down to the footer like the plain textarea (the editor library's
  wrapper between `composer.html.editor` and the content grows).
- `p1-views.spec.ts` waits for the first load of a freshly started `nail serve` through `openApp` in
  `e2e/support/views.ts` with an explicit 30 s timeout (`FIRST_LOAD_TIMEOUT`) instead of the default 5 s.

## Consequences

- Visual baselines of the `html` screen change for the dark themes (paper colours) and high contrast (forced colours).
- Quoting an HTML mail with inline images quotes `cid:` references that point nowhere in the reply (as before P1);
  forwards keep the parts as attachments.
