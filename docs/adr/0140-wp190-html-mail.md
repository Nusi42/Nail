# 0140 — WP190: HTML mail as its own document, and the mail view's responsive pass

- Status: accepted
- Date: 2026-10-05
- Wave: P1 (polish), package WP190 (contract ADR 0139)

## Context

Review finding: HTML mail did not render. The contract (ADR 0139, `docs/plan.md` "Wave P1") found the
causes: the sanitizer removed `<style>` blocks, `class` / `id` and the `<body>` attributes, so designed mail arrived
as unstyled text; `cid:` images were never resolved and a `srcdoc` frame inherits the page's CSP, which allows no
`cid:`; the reader's reading width (`68ch` on every top-level element) squeezed 600 px layouts and the `smart` dark
mode flattened every colour. Smaller findings: broken-image icons, HTML-only previews that ran blocks together,
and two nested scroll areas in the reader.

## Decision

**A mail body is a document of its own.** The reader always shows HTML in a sandboxed frame without scripts
(`sandbox=""` locally, the sandbox origin's `sandbox` CSP in the remote UI), so the mail's own rules cannot reach the
app. `nail_mime::sanitize_mail_html` (used by `parse_message_with` for every HTML and Markdown body) therefore keeps
more than the fragment sanitizer `sanitize_html` (plug-in HTML, Markdown blocks, quoting), which is unchanged:

- `class` and `id` attributes;
- every `<style>` block of the message (head or body, read with html5ever's tokenizer in `document.rs`, the
  tokenizer ammonia parses with; blocks inside comments, `<noscript>`, `<script>` and other raw-text elements are
  not collected; 256 KiB at most), filtered rule by rule in `css.rs`: comments and `<!--` / `-->` removed first
  (what is checked is what is emitted), selectors without `\`, `<`, braces or `url(`, declarations through the same
  filter as `style` attributes, `@media` / `@supports` kept three levels deep, every other at-rule dropped
  (`@import`, `@font-face` — fonts would load from anywhere —, `@keyframes`, `@namespace`, …), nested rules dropped,
  and no `<` in the result, so the sheet cannot close its element. The kept rules are placed in one `<style>` element
  at the start of the sanitized HTML;
- the `<body>` element's `bgcolor`, `text`, `link`, `background` (as rules before the mail's sheets: attributes lose
  against author CSS) and `style` (after them: it wins). Legacy colour values must be words or `#hex`;
- the declaration filter (inline and sheets) splits at `;` outside strings and parentheses (a `data:` URL stays
  whole), and now also drops `position: fixed` and generated text (`content` other than `""` / `none` /
  `normal`), so a mail cannot fake a link's address next to it — the link warnings judge the real text;
- remote `url()` references in sheets count as remote content (the bar's number) and are removed, or proxied after
  the user allowed them, exactly like attributes and inline styles.

**Inline images become `data:` URLs in nail-mime** (`inline_images` in `lib.rs`): an inline part whose Content-ID the
HTML names, at most 2 MiB (`MAX_INLINE_IMAGE_BYTES`) each and 8 MiB (`MAX_INLINE_IMAGES_BYTES`) per message, typed
by its first bytes (PNG, GIF, JPEG, WebP — never SVG, never the header's claim). References to missing or larger
parts lose their `src`; the part stays an attachment. `cid:` left `READER_CSP`; no transport needed a CSP change
(`img-src` already allows `data:` in the page, the desktop app and the sandbox document).

**The reader gives way to designed mail** (`ui/src/theme/reader.ts`). Colours sit at zero specificity (`:where`), so
the mail's `body` / `a` rules win. "Designed" is decided inside the frame with `body:has(> style, table, [bgcolor],
[background], [style*='background'])` (Chromium and WebView2 support `:has`): the reading width (`message.max-width`)
applies only to other mail, and the `smart` dark mode recolours only other mail; designed mail keeps its colours on a
light paper (`color-scheme: light`, white page) — the theme's `invert` / `off` modes are unchanged. Images that did
not load (blocked, missing, refused by the proxy) show their alt text in a dashed frame instead of the broken-image
icon (`img[alt]::after`, which Chromium draws only for images without content).

**One scroll area.** The frame cannot report its content height without scripts, so it fills the rest of the
reader (`flex: 1 1 auto`, at least 12 lines high) and scrolls the mail itself; the headers stay in place. The
parent marks designed mail (`message-view.body[data-state~="designed"]`, the same test on the sanitized string,
`isDesignedMail`) and widens the reader column for it (`45 × space.lg` plus the padding) so a 600–700 px newsletter
fits.

**Previews and text of HTML-only mail** (`html_text` in `lib.rs`) break lines after block elements and separate
table cells (mail-parser's converter breaks only at `<br>` and `</p>`).

**Responsive pass of the mail view** (`e2e/tests/p1-mail.spec.ts`, the P1 standard at all eight sizes, the narrow
layout's Folders / Messages / Message tabs included): the helper found one real fault — the reader's toolbar stayed
clipped (its More button off-screen) after the window was made wider and quickly narrower again, because the
toolbar ignored a jump back to the previous width as a measuring flip for good. It now checks again after the flip
window and fits when it still overflows (`Toolbar.tsx`, unit test in `m5bToolbar.test.tsx`). The screenshots showed
the rest: header values wrapped under their labels (now a two-column grid), and a narrow reader pane wasted 2 × 32 px
on margins (the reader's stack is a size container; below 640 px the column and the banners use the pane padding).

## Consequences

- Real newsletters and Outlook / Gmail HTML look as designed in `nail serve` (local and remote) and the desktop app;
  `e2e/tests/p1-mail.spec.ts` checks computed colours, inline and proxied images inside the frame on all three.
- `MessageBody.sanitized_html` can start with a `<style>` element and carry `class` / `id`. Consumers that embed it
  next to other content must sanitize it as a fragment again (the composer's quote does: `sanitize_outgoing`);
  MCP and plug-ins receive it as before (plug-ins never render it in the app's DOM).
- A mail can style anything inside its own frame (hide text, colour links). That is what every mail client allows;
  the protections that matter stay: no script, no network except the user-allowed proxy, no fixed overlays, no
  generated link text, the app's warnings outside the frame.
- New dependencies of nail-mime: `html5ever` 0.40 (already in the tree through ammonia, MIT / Apache-2.0) and the
  workspace's `base64` (MIT / Apache-2.0); `Cargo.lock` gains the two edges, no new crates.
