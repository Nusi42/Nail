# ADR 0193: HTML mail fills the reader

Status: accepted · 2026-10-15 · review finding (changes ADRs 0140, 0151)

## Context

Review finding: a rendered HTML mail stays at its minimum width when the app is resized. Two caps from the P1
and P2 skins caused it:

- The card feed (`thread-view mode="cards"`, ADR 0151) and its subject header were capped at the plain-text reading
  width (`READER_MEASURE` = 68ch plus padding, about 560 px). Every card, and so every HTML frame inside it, stayed at
  about 495 px however wide the reader pane grew; a 600 px newsletter always scrolled sideways.
- The single-message reader widened the column only for designed mail, and only to about 784 px (ADR 0140).

## Decision

- A message column with an HTML body (`message-view.column:has(> message-view.body)`, a child selector) has no
  maximum width: the frame fills the reader and follows the window, wider and narrower.
- The card feed and its header fill the reader pane.
- The reading width stays where it serves reading: plain-text bodies (`message-view.text`, `READER_MEASURE`) and,
  inside the frame, HTML mail without its own design (`reader.ts`, the theme's `message max-width`). Designed mail
  centres or stretches itself as its author wrote it.
- The maintainer's earlier decision stands: wide HTML mail is not scaled down to fit a narrow reader; it scrolls.

## Consequences

- On wide screens cards and their header row (sender, date, actions) span the pane, like other mail programs.
- `ui/src/theme/readerWidth.test.ts` pins the two rules; `p1-mail.spec.ts` "an HTML mail's frame follows the
  window's width" checks the frame at 1280 → 2400 → 1280 px. The visual baselines did not change (their reader panes
  are narrower than the old caps).
