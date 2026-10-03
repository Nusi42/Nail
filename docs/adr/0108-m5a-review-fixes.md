# ADR 0108: M5a review fixes: stack tracks for rendered children, visible switches and badges in High contrast, aligned reader banners, an icon on every default action

Status: accepted · 2026-10-03 · M5a review

## Context

The M5a reviews of the wave integrated by ADR 0107 confirmed these defects
(all checked against the running build before fixing):

- **Off switches invisible.** The off thumb was `bg` on a `bg-raised`
  track (1.14:1 in High contrast, about 1.2:1 in the dark themes); the
  track had a 1px outline, not the control border (2px in High contrast).
- **High contrast lost shapes.** `message-list.row:unread { border-left-width 6 }`
  did nothing (the skin's `border: none` leaves the style `none`), and the
  avatar / file-type tokens were black on the black panes, so avatars, file
  badges and tag dots lost their shape.
- **The attachment save status broke the card.** It was a fourth child of
  the three-column card grid and wrapped into the badge column of a new row.
- **Stack tracks went to the wrong children** (older than M5a, very
  visible with the reader column). Tracks were taken from `sizes` by child
  index, but a widget that renders nothing (config errors while all files
  are valid, the junk / security / body-status banners) still used one up:
  the reader's `message-view` got an `auto` track (an HTML body stuck in a
  150px iframe) and the root stack pushed the status bar out of the window.
- **Reader banners spanned the pane** while the header and body sit in the
  centred column.
- **60 of 99 default actions had no icon**, among them buttons of the
  default reader bar and the Contacts / Tasks buttons of the main toolbar.
- Low (also fixed): hover in High contrast was #141414 on black; unchecked
  check boxes had no 2px outline there; the focused message list showed only
  the browser's 1px ring; user themes showed an empty preview strip.

## Decisions

- **Stacks give tracks only to children that render.** `StackView` puts a
  hidden marker (`stack.slot`, `hidden`, so no grid item) before each child
  and, in a layout effect plus a `MutationObserver` on its direct children,
  writes `grid-template-rows` straight to the style manager (no React state,
  so a widget appearing later re-renders nothing) with `renderedStackTracks`
  (layout/engine.ts):
  the first element after a marker takes that child's track, further
  elements of the same child `auto`, children without elements none.
  `message-view` fills its row (`height: 100%`) and the iframe takes the rest.
- **Switches:** off = `fg-muted` thumb on a `bg` track with the control
  border (`--nail-border-control`); on = `accent-fg` on `accent`. Both pairs
  are in the contrast test's table (UI, 3:1; 7:1 in High contrast).
- **High contrast:** avatar and file-type fills are white with black text
  (`avatar-fg`); on the selected (yellow) row a black inset ring keeps the
  avatar's shape. Every row has a 6px left border, transparent when read and
  `accent` when unread, so text stays aligned. Hover is the `accent-soft`
  fill (selected items keep the highlight), check boxes get the control
  border as an outline, the message list a 3px focus ring. A new test checks
  the fills against every surface at 7:1.
- **Attachment status** sits in the card's text column next to the size
  (`attachment-card.text` is a two-column grid; the name spans both), so the
  card keeps its height; a failed save shows in `danger`.
- **Reader banners** (`security-message`, `security-warnings`,
  `remote-content`, `junk-verdict`, `sync-message-body`) get
  `width: min(68ch, 100% - 2 × space.xxl)` and `margin-inline: auto`: their
  coloured rule lines up with the subject and body.
- **Icons:** every action of `defaults/layout.kdl` has a Lucide icon
  (`agents.open-center` is now defined there too); 18 names joined
  `BUNDLED_ICONS` and the UI registry (lucide-react, ISC). A nail-config test
  fails for a default action without a bundled icon.
- **Theme previews:** a user theme previews with the palette of the built-in
  at the end of its `extends` chain, else with the built-in of its kind
  (`previewColors`). Its own colour overrides are not shown yet.
- **Visual harness:** three more screens — an HTML newsletter with its
  remote-content banner, a message with attachment cards, and the same after
  saving one — so 9 screens × 10 variants; all baselines regenerated and
  reviewed.

## Not changed (follow-ups for M5b, which reworks toolbars)

- In High contrast the main toolbar still wraps at 1280px ("Settings" on a
  second row), and in Compact the reader bar's second line can start with a
  separator. M5b's ranked toolbars with a "More" overflow replace wrapping.
- A user theme's preview does not show its own colour overrides (needs
  preview tokens in `ThemeInfo`).
