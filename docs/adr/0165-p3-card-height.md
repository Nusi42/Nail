# ADR 0165 (WP-card-height): mail frames as tall as their mail

Status: accepted · 2026-10-05 · P3 WP-card-height (the integrator renumbers)

## Context

ADR 0164 (P3 contract, section 3) decided how a sandboxed HTML mail frame
learns its content height without running mail scripts: local `srcdoc`
frames get `sandbox="allow-same-origin"` and the reader measures them; remote
UI frames on the sandbox origin run one hash-pinned script that posts the
height to the app origin named in a `parent` parameter. Until now an HTML
card kept a `max(12 × lg, 40vh)` minimum and the single-message reader's
frame filled the pane (`flex: 1 1 auto`), so short HTML mail showed a tall
empty box and long mail scrolled twice (frame and feed). This package
implements the design.

## Decisions

### The frames (`ui/src/widgets/MailFrame.tsx`)

- `MessageView`'s `Body` renders `LocalMailFrame` (srcdoc) or
  `RemoteMailFrame` (`render_url`); both are used by the single-message
  reader and by the cards of the conversation feed (the cards render
  `message-view`), so one change covers both.
- **Local:** the `srcdoc` is still set through the `nail-reader` Trusted
  Types policy (`setSrcdoc`). After each `load` of an `about:srcdoc` document
  (the initial `about:blank` is ignored) the reader adds, from the parent:
  capture listeners for `load` / `error` (images, fonts) and `toggle`
  (`<details>`) on the document, a `resize` listener on its window, a
  `ResizeObserver` on its root and body (width changes reflow the text), and
  `document.fonts.ready`. It only reads sizes and listens; it never writes
  into the document. Verified in Chromium and WebView2: listeners and
  observers of the parent run although the frame itself runs no script.
- **Remote:** the frame's `src` is `render_url?css=…&parent=<location.origin>`
  with `sandbox="allow-scripts"`. A `message` listener accepts a height only
  through `frameHeightOf` (its own frame's `contentWindow`).
- **Measurement** (`frameHeight.ts` `contentHeight`, the same arithmetic in
  the sandbox script): the root's `scrollHeight` when the document overflows
  the frame, else the root's own `offsetHeight` (so a frame can shrink after
  a width change), plus `innerHeight − clientHeight` (a horizontal scrollbar
  of wide mail stays below the last line). Clamped to `MAX_FRAME_HEIGHT`; a
  taller document scrolls inside.
- **Guard** (`FrameHeightGuard`): mail whose size depends on its frame's
  height would make "size the frame to the content" a loop. A flip-flop
  (A → B → A) settles at the larger height; 40 growths by the same step in a
  row (`min-height: 100vh` plus a margin) stop the growth. A new document
  (theme switch, remote content allowed) or a new frame width starts over.
- **Applying the height:** a geometry rule (`useGeometry`, `data-geom`), never
  an inline style; `data-state` gains `measured`. Until it is measured the
  frame has a placeholder height (`12 × lg`); after a new document the old
  height stays until the new one is measured (no jump on a theme switch).

### The skins

- `skin.ts`: `message-view.body` is `display: block; flex: 0 0 auto;
  height: calc(12 × lg)` (the placeholder) — never stretched to the pane or
  squeezed; the reader (`message-view`, `overflow-y: auto`) scrolls as a
  whole. The `max(…, 50vh)` rule the contract mentions no longer existed;
  the pane-filling `flex: 1 1 auto` was its successor and is gone.
- `threadViewSkin.ts`: the cards' `max(12 × lg, 40vh)` minimum is gone; the
  feed scrolls as a whole.

### The sandbox route (`crates/nail-server/src/sandbox.rs`)

- `HEIGHT_SCRIPT` (a constant; its CSP source `height_script_source()` =
  `'sha256-<base64>'`, computed once from the text with `sha2` / `base64`, so
  an edit of the script cannot desynchronize the hash) sits in the document's
  `<head>`. It reads `parent` from `location.search` (nothing → it does
  nothing) and posts `{type: "nail-frame-height", height}` to exactly that
  origin (never `*`) after `load`, image `load` / `error`, `toggle`, fonts,
  `resize`, and on `ResizeObserver` changes of the root and body, only when
  the height changed.
- `document_csp`: `script-src 'sha256-…'` (no `'unsafe-inline'`,
  `'unsafe-hashes'`, `'self'`), `sandbox allow-scripts allow-popups
  allow-popups-to-escape-sandbox` — never `allow-same-origin`.
- `GET /render/<token>?parent=…`: a `parent` that is not exactly one of the
  route's `frame-ancestors` sources answers **400** (`is_frame_ancestor`);
  without `parent` the document is served and the script stays idle.

### Deviation from the contract: the opaque origin

The contract's `frameHeightOf` accepted only `event.origin === sandboxOrigin`.
A sandboxed document without `allow-same-origin` posts from an opaque origin,
which `MessageEvent.origin` serializes as `"null"`, so no height would ever
be accepted. `frameHeightOf` now accepts the sandbox origin **or** `"null"`
(`OPAQUE_ORIGIN`), still only from the reader's own frame window — the check
that matters, since no other document can post as that window. No
`nail-api` change.

### Security notes (in addition to ADR 0164)

- The e2e tests prove on the real browser that a mail `<script>`, an inline
  `onerror`, a `javascript:` link, a meta refresh and CSS `expression()` do
  nothing — through the core's sanitizer on both paths, and on the local path
  even when the raw hostile document is written straight into the reader's
  frame (loopback `nail serve` only reports Trusted Types): the sandbox alone
  keeps it inert although the document has the app's origin; in the remote UI
  assigning a plain string to `srcdoc` throws (Trusted Types enforced).
- `allow-scripts` on the remote frame also re-enables "automatic features"
  such as meta refresh in that frame. The sanitizer removes `<meta>`, the
  app's `frame-src` (`'self' blob: data:` + the sandbox origin) limits where a
  frame may navigate, and a navigated frame keeps its sandbox (opaque origin):
  at worst it could post a (clamped) height. Mail scripts still cannot run
  in the sandbox document itself (hash-only `script-src`).

## Tests

- Vitest: `frameHeight.test.ts` (opaque origin, `contentHeight`, guard),
  `MailFrame.test.tsx` (sandbox values, srcdoc, measuring after `load` /
  image events / scrollbar, designed + measured states, re-measure after a new
  document, `parent` parameter, messages from other windows / origins
  ignored, clamping), `skin.test.tsx` (no 40vh, no stretching), and the
  sandbox-attribute assertions of `App.test.tsx`, `widgets.test.tsx`,
  `trustedHtml.test.tsx`.
- Rust: `sandbox.rs` unit tests (CSP with only the script hash, the script's
  contract constants and hash, `is_frame_ancestor`), `tests/transport.rs`
  (served CSP and script, `parent` accepted / refused with 400).
- Playwright `e2e/tests/p3-card-height.spec.ts`: short HTML card < 200 px and
  its frame < 120 px, the long newsletter in full (> 2000 px, no inner
  scroll), re-measure after "Load once" (a 560 px image), a narrower window,
  and a theme switch; the single-message reader without empty space; dark
  Graphite / Nocturne with the designed mail on its white paper; the remote
  UI on the sandbox origin; the hostile-mail checks; the desktop app
  (WebView2). Screenshots in `<build dir>/screens/p3/`. Existing specs
  (`mail`, `p1-mail`, `remote-content`) assert the new sandbox values and the
  measured frame.
- Visual baselines: only the `html` screen (the newsletter) changed — the
  40vh of empty paper below it is gone — in all ten variants.
