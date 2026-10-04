# ADR 0139: P1 contract — HTML mail root cause, responsive standard, shared helper

Status: accepted · 2026-10-04 · P1 WP190 (contract)

## Context

After M5 a review of the real app found three issues: controls
misalign and do not resize when the app window is resized, HTML mails did not
render, and the threading of emails is too cramped. Wave P1 (plan section 10,
"Wave P1 (polish)") fixes the first two and the open points from M5's verify
(the cosmetic list and the tests that are flaky under load). Threading gets a
separate UX proposal first and is not part of P1.

Four packages work in parallel: WP190 (mail view and HTML rendering), WP191
(window manager and every other window and dialog), WP192 (PIM views, shell
and the M5d editors) and WP193 (test stability). They need one root cause to
fix, one definition of "responsive" and one way to check it.

## Decisions

### The HTML root cause is in the sanitizer, not in the frame

The contract reproduced the report in `nail serve --seed demo`, `--seed
demo-multi` and the desktop app (WebView2 over CDP). The reader frame works in
all three: `srcdoc` through the `nail-reader` Trusted Types policy,
`sandbox=""`, a sized `message-view.body`. The demo corpus renders because it
uses only inline styles. Real HTML mail breaks because:

1. `nail-mime`'s sanitizer removes `<style>` and `<head>` with their content,
   drops `class` / `id`, and loses `<body>`'s attributes. A newsletter styled
   with a style sheet arrives as plain text.
2. `cid:` images are kept as references but never resolved. They are also
   blocked by the page CSP that an `about:srcdoc` document inherits
   (`nail-server` headers, Tauri's `csp`, the remote UI's sandbox document).
3. The reader CSS caps every top-level element at the reading width (68ch) and
   the `smart` dark mode wipes every colour.

The plan lists file and line for each. The fix belongs to WP190:

- keep style sheets through a CSS sanitizer with the inline-style rules
  (remote `url()` only via the proxy rewrite, no `@import`, no expressions /
  bindings, no `position: fixed`);
- keep `class` / `id`, and carry the body's attributes onto a wrapper;
- resolve `cid:` to size-capped `data:` URLs in nail-mime, so no transport's
  CSP has to change;
- restrict the reading width and `smart` dark mode to mail without its own
  layout.

The sandbox, Trusted Types, the proxy and remote-content blocking (ADRs 0032,
0038, 0041, 0048) stay as they are: style sheets run in the same script-less,
opaque-origin frame under the same `default-src 'none'` document CSP.

### The responsive standard

Every view, window and dialog is checked at widths 1600 / 1280 / 1024 / 800 ×
heights 1000 / 700. Windows are also checked at their min-size and resized
larger. The rules:

- no clipped text (an ellipsis or line clamp on purpose is fine);
- no overlapping controls within a window;
- no horizontal page scroll, and no vertical page scroll (panes and window
  bodies scroll);
- nothing off-screen sideways;
- toolbars overflow into More;
- form rows wrap with aligned labels;
- dialogs scroll their body, not the page;
- controls use the space they get.

### One shared helper: `e2e/support/responsive.ts`

The helper is a Playwright module rather than a Vitest utility, because layout
needs a real browser. It checks visible `[data-part]` elements and finds six
kinds of issue:

- `page-overflow` / `page-scroll`: it reads the scroll size of both the
  document and the body, because the app's body hides its overflow and would
  otherwise cut content off silently;
- `clipped`: overflow hidden / clip with content larger than the box, unless
  the box has `text-overflow: ellipsis` or a line clamp;
- `offscreen`: a control or text sticking out sideways, unless it is inside a
  horizontally scrolling container;
- `overlap`: controls and text leaves of the same layer whose visible
  rectangles (cut by their clipping ancestors) intersect. Each window, menu,
  listbox and tooltip is its own layer, so windows may overlap each other;
- `misaligned`: only for label groups a test names.

Elements of 1 px or less (screen-reader text) are skipped. `expectResponsive`
sweeps the sizes with a `prepare` hook per size, and saves
`<build dir>/screens/p1/<name>-<w>x<h>.png` for the screenshot review.
`resizeWindow` drags a window's `se` handle (the window manager clamps it to
its min-size). Specs may `ignore` subtrees or `skip` kinds, each with a
stated reason.

The helper only finds mechanical faults. Spacing, alignment and use of space
need the screenshots, and the packages review them.

`e2e/tests/responsive-helper.spec.ts` is the self-test. It uses static pages
and shows that each kind is found, that ellipses, line clamps, scrolled-away
content, other windows and 1 px text are not reported, and that the sweep
visits all 8 sizes and saves the screenshots.

### Slots

The packages work in existing folders, so the contract creates no stub
crates, feature folders or catalogs. One shared file is new:
`ui/src/theme/skinResponsive.ts` (`RESPONSIVE_SKIN_CSS`), owned by WP191 for
the form, button-row, card and window-body rules every dialog shares. The
theme compiler emits it after every feature skin and before the theme's part
rules, so it can adjust the shared primitives and `theme.kdl` still wins
(`skinResponsive.test.ts`).

To keep the packages from colliding:

- WP190 owns the mail sections of `theme/skin.ts`;
- WP192 overrides the Appearance rules of `theme/skin.ts` in its own skin
  instead of editing them;
- only the integrator regenerates the visual baselines and the default-theme
  fixture.

## Consequences

- Each package has a precise target. WP190 starts from file:line facts, not
  from a repro hunt.
- One helper means one definition of "responsive", and its screenshots make up
  the review set for the maintainer's human gate.
- The helper does not decide aesthetics, so the packages must look at their
  screenshots. The reviews check that they did.
- Keeping mail style sheets widens what the sanitizer must police (CSS
  exfiltration via `url()`, `@import`, fonts). WP190's CSS sanitizer tests and
  the quality review cover it.
