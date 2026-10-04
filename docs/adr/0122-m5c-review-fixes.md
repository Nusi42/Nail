# ADR 0122 — M5c review fixes

- Status: accepted (2026-10-04)
- Context: M5c (window manager, non-modal windows, desktop pop-outs; ADRs 0116–0121), review of the integrated wave.

## Decisions

1. **Pop-out pages have a window layer.** `PopOutHost` renders a `WindowLayer` with `main=<its window>`. The pop-out
   itself is the page's main window. Windows its content opens (`composer-discard:<key>`, `composer-close:<key>`,
   `plugin-send-veto`, plug-in dialogs) are drawn as page-modal windows over it. Before this, they were added to the
   pop-out's UI store but never drawn, so Discard and Escape silently did nothing. The pop-out's "content closed →
   `ClosePopOut`" listener now decides in a microtask from `store.getState()`. Before, it used the listener argument,
   so it could see a change that the composer session sync takes back in the same update (a composer with unsaved
   changes stays open and asks).
2. **Closing a pop-out's OS window asks the page.** In the desktop shell, `CloseRequested` on a `popout-*` window is
   prevented, and the page gets the DOM event `nail-pop-out-close-requested` (`POP_OUT_CLOSE_EVENT`, through
   `WebviewWindow::eval`). The page closes its window the way the in-page × button does: a composer with unsaved
   changes asks save / close without saving / keep editing, and a saved one closes at once. The OS window closes when
   the content is closed (`ClosePopOut` → `destroy`, which does not raise `CloseRequested` again). While a question is
   open, further close requests are ignored. If the page cannot be asked (`eval` fails), the OS closes the window.
   Follow-up: there is no time-out for a page that hangs; closing the main window still closes every pop-out.
3. **Popping out is opt-in.** `DEFAULT_WINDOW_SPEC.pop_out` is now `false` (Rust and TS). Each webview has its own UI
   and feature stores, and only the composer hands its state over (its preparer saves the draft). Editors of the main
   page's selection (event / contact / task editors, Apply labels, Classify, …) opened empty in a pop-out, and the
   in-page window with unsaved input was closed. `defaults/layout.kdl` opts in the windows that show core data alone:
   `composer`, `settings`, `rules`, `outbox`, `snoozed`. A feature may opt in with `pop_out: true` in its
   registration once it registers a preparer, or once it needs none. `PopOutRouter` re-pops only kinds whose spec
   allows it, so a kind that was popped out before this change stays in the page.
4. **Owned questions sit over their owner.** A modal window that belongs to a window opens centred on that window's
   shown rectangle (`centredOn`; `WindowFrame.anchor`, read once from the owner's `WindowFrameHandle.rect()` when the
   owner is known). It stacks just above its owner (`stackingOrder`: owner z + 1), so a window the user raises covers
   both, as an owned dialog does on Windows. Page-modal questions keep their place above every window. A remembered
   position of the question kind is ignored for owned questions.
5. **Cascade slots.** A new window takes the first cascade slot that no open window of its kind uses
   (`cascadeSlots`). It keeps that slot while open. Open A and B, close A, open C: C opens in A's place, not exactly
   over B.
6. **What sending needs sticks with Send.** The finding was half right. The high-contrast / comfortable composer did
   not squeeze its rows: its content was taller than the 820 × 720 window, the composer scrolled (headless Chromium
   hides scrollbars), and the sticky actions row covered the attachment reminder. The reminder, an error and the
   send-later row now sit in a new sticky `composer.footer` together with `composer.actions`, so only the message text
   scrolls under the footer. The composer's other rows also get `flex-shrink: 0` (`[data-part="composer"] >
   :not([data-part="composer.body"])`), so a short window scrolls instead of squeezing them. The high-contrast composer
   baselines (both densities) were regenerated. The other themes stay within the comparison threshold.
7. **Pop-outs open at the window's spec size.** `PopOutWindow` gains optional `size` / `min_size` (CSS pixels, from
   the resolved window spec). They are normalised (`pop_out_sizes`) and passed through `PopOutOpen`. The shell places
   a first pop-out at that size (`place_sized`) and never lets it get smaller than the spec minimum (but not below
   `MIN_POP_OUT_SIZE`). A remembered pop-out geometry still wins for the first size.
8. **Keyboard resize never shrinks when growing.** Shift+Right / Shift+Down on a window that stands off the right or
   bottom page edge keeps its size, where it used to jump to the visible width. Shift+Left / Shift+Up shrink it by one
   step from its real size.
9. **Autosave test hook.** `autosaveDelay()` reads the global `__nailAutosaveMs` (`setAutosaveDelay` in unit tests;
   `page.addInitScript` in Playwright). Tests of the "unsaved changes" flows lengthen the delay so the autosave cannot
   race them. The production default stays `AUTOSAVE_MS` (1500 ms).
10. **Test control `close-pop-outs`** (seeded desktop runs only): calls `WebviewWindow::close` on every pop-out, which
    raises `CloseRequested` like Alt+F4. `desktop-m5c.spec.ts` uses it.

## Not changed

- Resize handles stay 6 px strips inside the frame (`overflow: hidden`). Moving them outside needs an inner wrapper,
  which changes every window's structure and baselines. This is a follow-up; the body scrollbar under the `e` / `se`
  handles is a known limitation.
