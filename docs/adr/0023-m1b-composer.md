# ADR 0023: Rich composer — nail-compose and the composer feature

Status: accepted · 2026-09-29 · M1b WP21

## Context

WP21 builds the composer of the design's "Composer: HTML and Markdown"
section on the M1b contract (ADR 0022): the MIME / Markdown / quoting
engine in `crates/nail-compose` and the UI in `ui/src/features/composer/`.
In the parallel stage WP21 owns only those two places plus
`locales/{en,de}/composer.ftl` and the npm dependencies; the core request
handlers, the default layout / theme and shared mocks are wired by WP25
(see "Integration" below).

## Decisions

### nail-compose (Rust)

- **Pure library, no I/O.** Attachment bytes come through the `BlobSource`
  trait (any `Fn(&str) -> Option<Vec<u8>>` works); `Date` and `Message-ID`
  are passed in (`MessageMeta`), so output is deterministic and testable.
  Localized texts (quote header, forward intro and field labels) are
  passed in as well (`QuoteStrings`): the core formats them from Fluent.
- **MIME with mail-builder 1.0**, structure built by hand with `MimePart`:
  plain → `text/plain; format=flowed` (own RFC 3676 encoder: 72 columns,
  soft breaks, quote depth kept, space-stuffing, "-- " untouched);
  HTML → `multipart/alternative [text/plain (HTML → text), multipart/related
  [text/html, cid images]]`; Markdown → `multipart/alternative [text/plain
  (the source), text/markdown; variant=GFM (optional, RFC 7763),
  multipart/related [text/html, …]]`; attachments → `multipart/mixed`.
  Inline images the HTML does not reference (and all images of a plain
  message) are sent as normal attachments. Bcc (draft + identity) only in
  the envelope. Unknown / malformed MIME types become
  `application/octet-stream`; CR/LF in header text becomes a space.
- **Markdown with comrak 0.55** (CommonMark + table, strikethrough,
  autolink, task lists, footnotes; raw HTML allowed and then sanitized).
  Code blocks are highlighted by syntect through comrak's `SyntectAdapter`
  **with a theme**, i.e. inline `style="color:…"` spans, so highlighting
  survives clients that strip `<style>`; the theme is per identity
  (`MailStyle::code_theme`, one of syntect's bundled themes, unknown names
  fall back to `InspiredGitHub`). Adapters are cached per theme. A
  signature after the last "-- " line is rendered separately with hard
  line breaks inside `<div class="signature">`.
- **Mail stylesheet**: `MailStyle { css, code_theme }`; `mail_document`
  wraps a fragment in a full document and inlines the CSS with css-inline
  (no remote loading, `<style>` removed; element styles win).
  `DEFAULT_MAIL_CSS` gives readable defaults. `RenderPreview` answers with
  exactly this document, so preview and sent HTML are identical.
- **Sanitizing with ammonia** (`sanitize_outgoing`): an allowlist for mail
  (formatting, tables with spans/widths, `style`/`class`/`id`, images with
  `http(s)`/`cid`/`data:image` sources, checkbox inputs for task lists);
  scripts, forms, frames, handlers, `<style>`, `javascript:` and
  non-image `data:` URLs are removed. Applied to outgoing HTML, quoted
  originals and HTML signatures.
- **HTML → text / Markdown: own walker over dom_query** (0.28, already in
  the tree through Tauri, same html5ever as ammonia) instead of html2md /
  htmd, because the composer must *report* what a conversion drops
  (`Conversion::dropped`: `style`, `underline`, `font`, `color`,
  `alignment`, `table-layout`, `image-size`, `super-subscript`,
  `highlight`). Markdown output escapes specials, uses GFM tables for simple
  tables, fenced code with the language, `\` hard breaks, `<url>` autolinks.
  `has_formatting` decides whether a rich → plain switch is lossy.
- **Quoting** (`quote` module): plain "> " (nested `>>`), HTML
  `<blockquote type="cite">` with the sanitized original, Markdown `>`
  over the converted original; top or bottom posting (`Posting`), the
  signature placed accordingly. Forwards carry intro + header fields and
  the original unquoted; `new_draft` copies the original's attachments.
- **Drafts** (`draft` module): `new_draft` (identity mode or the requested
  one, signature variant of the mode, reply / reply-all recipients without
  the user's own addresses, `Re:` / `Fwd:` without doubling) and
  `convert_draft` (plain → any and Markdown → HTML lossless; HTML →
  Markdown best effort; rich → plain lossy when formatted).
- **Signatures**: `signature_block` picks the variant of the mode and
  derives a missing one (HTML → text / Markdown, Markdown → HTML, text →
  HTML).
- **Templates**: `{name}` placeholders (`[A-Za-z0-9._-]`, `{{`/`}}`
  escapes); unknown names are an error listing all of them;
  `expand_template_html` escapes values.
- **Tests**: unit tests per module; `tests/mime.rs` golden files
  (`tests/golden/*.eml`, boundaries normalized; regenerate with
  `NAIL_UPDATE_GOLDEN=1 cargo test -p nail-compose --test mime`) plus
  round trips through `nail-mime` (headers incl. encoded words, threading,
  inline image by cid, attachment bytes, flowed text).

### Composer UI (`ui/src/features/composer/`)

- The layout kind stays `composer`: `widgets/Composer.tsx` re-exports the
  feature's widget, so layouts and the M1a tests keep working and no
  shared registry changes are needed.
- **State split.** The envelope (identity, recipients, subject, plain text,
  context, draft id, busy / error) stays in `uiStore.composer`; the rich
  part (mode, HTML, Markdown, attachments, `markdown_part`, signature
  choice, preview / source toggles, lossy switch, autosave, reminder,
  uploads, inline previews) lives in a feature store per UI store
  (`richStoreFor(uiStore)`), reset when a composer session starts unless a
  flow primed it (`primeRich`). Plain-text drafts are therefore identical
  to M1a.
- **Flows** (`flows.ts`): `startRichCompose` / `startRichReply` use
  `NewDraft` and fall back to the M1a flows when the core answers
  `unsupported`; `saveRich` (serialized per store, `quiet` for autosave),
  `sendRich` (validation, upload check, attachment reminder with "send
  anyway"), `switchMode` via `ConvertDraft` with a confirmation when
  `lossy`, `uploadFiles` via `UploadAttachment` (base64; inline images get
  a `<uuid>@nail` content id and a local data-URL preview), signature swap
  on identity change (plain / Markdown).
- **Keys.** The widget matches the layout's `composer` key bindings itself
  (`KeyMatcher`) and runs the rich flows for `composer.send` /
  `composer.save`, preventing the global dispatcher from running the
  plain-text built-ins. Buttons / menus that dispatch the built-in
  `composer.*` actions still reach the M1a flows until WP25 points them
  here (see Integration).
- **Autosave**: the first draft key of a session is the baseline; after a
  change, `SaveDraft` runs 1.5 s after the last edit (`AUTOSAVE_MS`).
- **HTML editor**: TipTap 3 (StarterKit incl. link / underline, Image,
  TableKit without column resizing), `injectCSS: false` (its base styles
  belong in the theme), toolbar of `data-part="composer.format.button"`
  with `data-command`, link form, image upload, table row / column
  commands, raw source view (textarea; the editor stays mounted). Stored
  HTML uses `cid:`; previews are swapped in and out (`toDisplayHtml` /
  `toStoredHtml`).
- **Markdown editor**: CodeMirror 6 (history, default keymap without
  `Mod-Enter` / `Mod-s`, markdown language, default highlight style, line
  wrapping); `data-part` on the editor and content elements via
  `editorAttributes` / `contentAttributes`. CodeMirror injects its own
  structural base theme (library internals, not our components).
  Preview: `RenderPreview` (300 ms debounce) in a sandboxed iframe.
- **Autocomplete**: the To / Cc / Bcc inputs stay `textbox`es (accessible
  names unchanged) with `aria-autocomplete="list"`, a `listbox` of
  `SuggestAddresses` results for the entry being typed; arrows, Enter / Tab
  to accept, Escape closes the list without closing the window.
- **Attachment reminder**: whole-word keywords from
  `composer-attachment-keywords` (per language), ignoring quoted lines,
  quoted HTML and the signature.
- **Mock**: `features/composer/mock.ts` answers the five composer requests
  (a small Markdown renderer and converter imitate nail-compose);
  `saveRichDraft` / `acceptRichDrafts` store rich drafts because the shared
  `api/mockOutbox.ts` still refuses them.

### Dependencies

- Rust: `dom_query = "0.28"` (no default features) in nail-compose only;
  `thiserror`, `sha2`, `hex` from the workspace. No aws-lc, no C builds.
- npm (ui): `@tiptap/{core,pm,react,starter-kit,extension-image,extension-table}` 3.31,
  `@codemirror/{state,view,commands,language,lang-markdown}` 6.x. The main
  chunk grows by ~0.5 MB; lazy-loading the two editors is a follow-up.

## Integration (WP25)

1. **Core** (`nail-core/src/composer.rs`, `outbox.rs`, `strings.rs`):
   - `SaveDraft`: drop the `uses_rich_content()` refusal; sanitize
     `draft.html` with `nail_compose::sanitize_outgoing` on save; record
     `draft_attachments` for the blob GC.
   - Sending: replace `crate::mime::build_plain_text` with
     `nail_compose::build_message(&draft, &identity, reply, MessageMeta {
     date, message_id }, &style, &blobs)` where `reply` is
     `ReplyHeaders { message_id, references }` from `message_meta`,
     `style` the identity's `MailStyle` (default until the contract has
     one) and `blobs` a closure over the attachment bytes read from the
     blob store beforehand; `nail_compose::new_message_id` replaces
     `mime::new_message_id`. Map `ComposeError::MissingBlob` to
     `not_found`, `NoSender` to `invalid_request`. Then delete `mime.rs`.
   - `NewDraft`: choose the identity like `ChooseIdentity`, build
     `OriginalMessage` from the stored body (text, sanitized HTML,
     headers; for forwards copy attachments into the blob store as
     `DraftAttachment`s), `QuoteStrings` from Fluent (`composer-quote-header`
     with `$date` / `$name`, `composer-forward-header`, `message-header-*`
     labels — load `compose.ftl` / `composer.ftl` in `strings.rs`), own
     addresses = all identities' emails, then `nail_compose::new_draft`.
   - `RenderPreview`: `render_markdown_document(markdown, &style)`.
   - `ConvertDraft`: `convert_draft(&draft, mode, &style)` →
     `DraftConverted { draft, lossy: converted.lossy() }`.
   - `UploadAttachment`: decode base64, size limit → `too_large`, store the
     blob, answer `DraftAttachment { filename, mime, size, blob, content_id }`.
   - `SuggestAddresses`: `Store::suggest_addresses(prefix, limit)`; record
     addresses (`record_addresses`) on send and on ingest.
   - Autosave to the server Drafts folder (store columns `server_*`, v4)
     is core work as well: APPEND the built message to Drafts after a save
     (debounced in the core), replace the previous copy.
2. **Actions** (`actions/registry.ts`): point `mail.compose`,
   `mail.reply`, `mail.reply-all`, `mail.forward`, `composer.send`,
   `composer.save`, `composer.discard` and `outbox.undo`'s reopen to
   `startRichCompose`, `startRichReply`, `sendRich`, `saveRich`,
   `discardRich`, `openRichDraft` (exported from
   `features/composer/index.ts`); then `state/compose.ts` keeps only the
   shared helpers.
3. **Shared mock**: route rich `SaveDraft` to
   `features/composer/mock.ts#saveRichDraft` in `api/mockOutbox.ts` (and use
   `markdown` / `html` text for the Sent copy preview); remove
   `RenderPreview` from the "unsupported until they land" list in
   `api/mock.test.ts`.
4. **Contract** (nail-api, accounts.kdl): per-identity `mail_style { css
   code_theme }`, `posting "top" | "bottom"` and optionally a quote-header
   template; a way to show inline images of reopened drafts (blob URL or a
   `GetBlob` request) — the editor shows alt text until then.
5. **Layout / theme** (`defaults/*.kdl`): actions `composer.set-mode`
   (arg `plain|html|markdown`), `composer.toggle-preview`,
   `composer.toggle-source`, `composer.attach` with labels
   `@composer.action.set-mode` … (strings in `composer.ftl`); icons
   `format-<command>` and `attachment` optional. Theme parts to add:

```kdl
part "composer.toolbar" { display "flex"; align-items "center"; gap "$space.sm"; }
part "composer.modes" { display "inline-flex"; border "$border.thin"; border-radius "$radius.sm"; }
part "composer.mode" { padding "$space.xs" "$space.sm"; border "none"; background-color "transparent"; color "$color.fg"; }
part "composer.mode[state=\"selected\"]" { background-color "$color.bg-raised"; font-weight "600"; }
part "composer.option" { display "inline-flex"; align-items "center"; gap "$space.xs"; font-size "$font-size.sm"; }
part "composer.address" { position "relative"; }
part "composer.suggestions" { position "absolute"; z-index 10; margin 0; padding "$space.xs" 0; list-style "none"; border "$border.thin"; border-radius "$radius.sm"; background-color "$color.bg"; }
part "composer.suggestion" { display "flex"; gap "$space.sm"; padding "$space.xs" "$space.sm"; cursor "pointer"; }
part "composer.suggestion[state=\"active\"]" { background-color "$color.bg-raised"; }
part "composer.suggestion.email" { color "$color.fg-muted"; }
part "composer.format" { display "flex"; flex-wrap "wrap"; gap "$space.xs"; }
part "composer.format.button" { padding "$space.xs" "$space.sm"; border "$border.thin"; border-radius "$radius.sm"; background-color "transparent"; color "$color.fg"; }
part "composer.format.button[state=\"active\"]" { background-color "$color.bg-raised"; }
part "composer.html" { display "flex"; flex-direction "column"; gap "$space.xs"; flex-grow 1; min-height 0; }
part "composer.html.content" { min-height 200; padding "$space.sm"; border "$border.thin"; border-radius "$radius.sm"; white-space "pre-wrap"; overflow-wrap "break-word"; outline "none"; }
part "composer.html.source" { min-height 200; font-family "$font.mono"; }
part "composer.link" { display "flex"; align-items "center"; gap "$space.sm"; }
part "composer.markdown" { display "grid"; grid-template-columns "minmax(0, 1fr)"; gap "$space.sm"; flex-grow 1; min-height 0; }
part "composer.markdown[state=\"split\"]" { grid-template-columns "minmax(0, 1fr) minmax(0, 1fr)"; }
part "composer.markdown.editor" { min-height 200; border "$border.thin"; border-radius "$radius.sm"; font-family "$font.mono"; }
part "composer.preview" { display "flex"; min-height 200; border "$border.thin"; border-radius "$radius.sm"; }
part "composer.preview.frame" { flex-grow 1; border "none"; background-color "#ffffff"; }
part "composer.preview.error" { color "$color.danger"; padding "$space.sm"; }
part "composer.lossy" { display "flex"; align-items "center"; gap "$space.sm"; padding "$space.sm"; border-radius "$radius.sm"; background-color "$color.bg-raised"; }
part "composer.reminder" { display "flex"; align-items "center"; gap "$space.sm"; font-size "$font-size.sm"; color "$color.fg-muted"; }
part "composer.reminder[state=\"confirm\"]" { color "$color.danger"; }
part "composer.attachments" { display "flex"; flex-direction "column"; gap "$space.xs"; }
part "composer.attachments.list" { display "flex"; flex-wrap "wrap"; gap "$space.xs"; margin 0; padding 0; list-style "none"; }
part "composer.attachment" { display "inline-flex"; align-items "center"; gap "$space.xs"; padding "$space.xs" "$space.sm"; border "$border.thin"; border-radius "$radius.sm"; }
part "composer.attachment.size" { color "$color.fg-muted"; font-size "$font-size.sm"; }
part "composer.attachment.inline" { color "$color.fg-muted"; font-size "$font-size.sm"; }
part "composer.uploading" { color "$color.fg-muted"; font-size "$font-size.sm"; }
part "composer.saved[state=\"failed\"]" { color "$color.danger"; }
```

## Consequences

- Until WP25 wires the core, the real app answers the composer requests
  with `unsupported`: `startRich*` fall back to the M1a flows, mode
  switches show the error, and rich drafts are refused by `SaveDraft`.
  The mock (`?mock=1`, Vitest) has the full behaviour.
- `api/mock.test.ts` expects `RenderPreview` to be `unsupported`; it fails
  now that the composer mock answers it (item 3 above).
- The composer supersedes `nail-core/src/mime.rs` once integrated.
