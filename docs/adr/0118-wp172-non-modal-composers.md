# ADR 0118: non-modal windows, several composers (M5c WP172)

Status: accepted · 2026-10-04 · M5c WP172

## Context

Plan section 10, wave M5c: the composer, Settings, Rules, the plug-in
manager and the other tool windows become non-modal; only confirmations
stay modal; several composers can be open at once. The contract (ADR 0116)
gives window ids with instances (`composer:<key>`), `WindowSpec` resolved
from defaults → `Feature.windows` → the layout, and `UiState.composers`
next to the single `UiState.composer`. It left two decisions to WP172: how
the composer moves onto instance ids, and how the single-composer readers
outside the composer feature (sign / encrypt toggles and send guard,
plug-in context and panel, MCP UI context, palette contexts, toolbar
`enabled`) find "the" composer. WP172 may edit only `features/composer/`
and the `windows` registrations in the other features.

## Decisions

### One window per draft

- Every new message, reply, forward, `mailto:` link, program draft and
  reopened draft opens `composer:<key>` (`features/composer/sessions.ts`,
  `openComposer`): the key is the draft id when it is a valid instance key
  and free, else `w1`, `w2`, … (`newWindowInstance`). The key stays when the
  first save gives the draft an id; `findComposerByDraft` looks drafts up by
  `composers[id].draftId`, so opening an open draft (Outbox → Continue
  writing, Undo send) raises its window instead of opening a second one.
  "Resume the unfinished message" (M1a) is gone.
- Envelope in `UiState.composers[<id>]`; rich state (mode, bodies,
  attachments, autosave) in one store per window
  (`richStoreFor(ui, id)`), dropped when the window closes (inline-image
  previews are kept per UI store, so an undone send shows its images).
- Components find their composer through `ComposerWindowContext`
  (`useComposerWindowOf()` = the window instance when it is a composer
  instance, else the plain `composer`). The flows take `{ window }`;
  without it they act on the focused composer (`composerTarget`).
- The M1a fallback flows of `state/compose.ts` (core without `NewDraft`)
  run on a scratch UI store; what they compose opens in a new window, so a
  fallback never overwrites an open composer.
- A pop-out can open a draft in a given window id (`openRichDraft(ctx, id,
  { window: "composer:<key>" })`) to match its `#/window/<id>` route.

### `UiState.composer` = the focused composer (mirror)

While instance composers are open, `UiState.composer` mirrors the topmost
`composer:<key>` in the z-order (the window manager raises a window on
click / focus). The single-composer readers keep working unchanged and act
on the composer the user works in — no edits outside the slot, and the
same answer for toolbar `enabled`, palette contexts, MCP and plug-ins. A
write to the mirror (an older flow) goes to the focused composer;
`composer = null` closes it. Without instance composers `composer` is the
plain `composer` window's own state (a layout's `window.open "composer"`,
tests); a plain composer with a message moves into an instance window when
another composer opens. This replaces ADR 0116's sentence "`UiState.composer`
stays the composer of the window `composer`" (the doc comment in
`state/uiStore.ts` needs the update; request to WP174).

The sign / encrypt choice lives in one store (`features/security`). It
belongs to the focused composer: on a focus change the outgoing choice is
kept per window and the incoming one restored; a draft's saved `crypto`
applies when its window is focused. Composers in the background save and
compare drafts with their kept choice (`cryptoFor`, `richDraftOf(c, r,
choice)`), so focusing another composer never triggers an autosave or
saves the wrong choice. Sending from a background window focuses it first.

### Closing asks only when something is unsaved

- `isUnsaved(id)`: the draft key differs from the last saved (or opened)
  one. Autosave runs 1.5 s after the last change, so a composer usually
  closes at once and its draft stays under Drafts.
- The window layer closes windows with `closeWindow` (× button, Escape /
  `window.close`, a view switch). The session sync sees an instance window
  disappear; when it is unsaved and no flow closed it (`markClosing`: sent,
  discarded, confirmed) it puts the window back in the same store update
  (React never unmounts it) and opens the modal `composer-close:<key>`:
  **Save draft** (then close) / **Close without saving** / **Keep editing**
  (focused). A proper close guard in the window layer would be cleaner; it
  is requested from WP174 (the current way needs no contract change).
- The Discard button asks through the modal `composer-discard:<key>` when
  there is a saved draft or unsaved changes; an untouched new message goes
  at once. The `composer.discard` action stays immediate (palette / keys).

### Modality and sizes (`Feature.windows`)

Registered by each feature (`windows` export next to `dialogs`):

- **Confirmations, modal** (`confirmation: true`): `composer-close`,
  `composer-discard` (both `instances: "multiple"`), `exchange-mailbox-remove`,
  `exchange-category-delete`, `gmail-label-delete`, `pim-remove-calendar`,
  `pim-edit-scope` (this occurrence or the series — before a delete / move),
  `scheduling-cancel-meeting` (sends cancellations; resizable, 520 × 460),
  `smartcard-pin` (an operation waits for the answer). The plug-in veto's
  "Send anyway" is the plug-ins feature's own modal dialog (not a window).
  "Delete permanently?" has no dialog yet (the action deletes at once) —
  requested from WP174.
- **Everything else non-modal** with a default and a minimum size: the
  composer (780 × 640, min 520 × 400, `instances: "multiple"`), rules
  (900 × 640), the plug-in manager (900 × 640), the confirmation center, the
  wizards, editors and pickers (sizes in each `index.ts`). The command
  palette does not pop out. A test checks every feature dialog has a
  registration and that exactly the confirmations resolve modal, layout
  windows included.

### Window title, cascade, reflow

- `windowTitles.composer(state, id)` (composer `index.ts`) gives the
  subject, `null` = "New message"; the window layer (WP171's) still shows
  `window-<kind>` — wiring requested from WP174.
- New windows cascade by `WINDOW_CASCADE_OFFSET` from the previous one —
  the window manager's job (ADR 0116); the composer only opens windows.
- `features/composer/skin.ts` (`COMPOSER_SKIN_CSS`, tokens only): the
  composer is a size container (`container: nail-composer / inline-size`);
  the footer (Send / Save / Discard) is sticky at the bottom of the
  scrolling composer, so nothing is clipped at the minimum size; below 36rem
  labels go above fields, the Markdown preview below the source and the
  editors start lower; below 26rem the footer wraps as one group. The
  responsive rules use two-part selectors so they win over the theme's
  single-part composer rules only inside the queries. Emitting it from
  `theme/compile.ts` (like the settings / PIM skins) is requested from WP174.

## Consequences

- Several composers edit, save and send independently; the readers outside
  the feature follow the focused one. Known limits until WP174 wires the
  requests: the `security-composer` widget and the program-draft notice in
  a background composer window show the focused composer's state (they read
  `UiState.composer`); the window title is the kind's text.
- Tests outside the slot that expected the window id `composer` or the
  "resume" behaviour need the patches listed in the WP172 report (verified
  on copies).
- Strings: `composer.ftl` (titles `window-composer-close` /
  `window-composer-discard`, the confirmation texts); `de` stays empty.
