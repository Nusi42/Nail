# ADR 0181: windows follow the app's size

Status: accepted · 2026-10-08 · review finding (desktop test of 4290474); amends ADRs 0117, 0141

## Context

Finding: resizing the app did not resize the compose / read windows. Reproduced in `nail serve` and the desktop
app: a window opened while the app was small (a composer opened in a 1000 × 600 window) was fitted to the page
when it opened, and that fitted rectangle became its geometry. Making the app larger left it at the cramped
size and place; only maximized and snapped windows followed the page. Shrinking already worked (ADR 0141), the
mail view's panes reflow (`fr` sizes), and there is no separate reader window — the reader is a pane.

## Decisions

The window manager tells apart where a window's place came from:

1. **Maximized and snapped windows** fill the page (or its half) whatever its size — unchanged.
2. **A window the user has not placed** (not moved, resized, maximized, snapped or stepped by keyboard since it
   opened) is placed by the window manager for the page it is on *now*: its default rectangle (the spec's size,
   centred, cascaded) or its remembered one, fitted to the page and moved wholly onto it. When the page grows it
   gets its full size back and is centred again (default) or returns to its remembered place. `WindowFrame`
   keeps the user's geometry (`placed`) apart from the opening inputs (the remembered geometry taken once when
   the states load, the owner's anchor, the cascade slot) and derives the shown geometry from them for every
   page size.
3. **A window the user placed** keeps its size and place. On a smaller page it shrinks to the page and moves in
   (wholly on it if it lay wholly on the page when placed, else `WINDOW_MIN_VISIBLE` px stay, ADR 0141); when
   the page grows again it gets the user's size and place back.
4. Nothing is remembered because of a page resize (`window-state.json` changes only on the user's moves).
5. Pop-out windows are OS windows: their page fills the OS window (`pop-out`: fixed, inset 0) and the composer
   inside reflows with it. The desktop smoke test resizes the main window and the pop-outs through the test
   control `resize-main:<w>x<h>` / `resize-pop-outs:<w>x<h>` (`nail_test_shell`, seeded runs only).

## Consequences

Tests: `WindowLayer.test.tsx` (opened small then grown, remembered geometry on a smaller page, maximized at
every size, P1's moved-in rule now for a placed window), `e2e/tests/app-chrome.spec.ts` (composer at several
page sizes, a user-placed window, panes reflow) and `desktop-m5c.spec.ts` (the OS windows resized).
