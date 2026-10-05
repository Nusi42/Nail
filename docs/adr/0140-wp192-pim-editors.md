# ADR 0140: P1 WP192 — calendar, contacts, tasks and the M5d editors at every window size

Status: accepted · 2026-10-05 · P1 WP192 (parallel package; the integrator renumbers)

## Context

ADR 0139 defines the P1 responsive standard (8 viewport sizes, windows at their min-size and
resized larger, no clipped text, overlaps, page scroll or off-screen controls) and the helper
`e2e/support/responsive.ts`. WP192 owns the calendar, contacts and tasks views, the navigation
rail and view host, customize-layout mode, the toolbar customizer, Settings → Appearance and the
theme editor, plus the M5 cosmetic list (a duplicated theme keeps the source's header comment, the
toolbar customizer clips the italic "Separator", the theme editor shows its title twice, "This
theme" tags wrap unevenly).

The first sweep (`e2e/tests/p1-views.spec.ts`) found: the calendar pane pushed past the window at
800 px (the sidebar is a fixed `260px` pane and the calendar pane has `min-sizes` 480 px, and the
calendar's toolbar widened its pane); event times squeezed into half a line under long titles;
free-time slot labels cut without an ellipsis; customize-layout mode cutting off the reader pane at
1024 px; the toolbar customizer's lists collapsing onto each other at its min-size (the narrow
rule was a viewport `@media` query); long token values cut in the theme editor. The screenshots
showed more: the contact filters not using the list's width, a contact card stretched over 1,200 px
with Edit / Delete far from the name, the light / dark theme fields with a separator line over one
of them only, a cramped preview beside the token list.

## Decisions

- **The calendar is a size container** (`container: nail-calendar / inline-size` in `parts.ts`):
  its content never widens the pane it sits in. Below 40rem of calendar width the toolbar takes two
  rows (buttons, then the range on its own line), the view menu's "View" label and the "New event"
  text are visually hidden (both keep their accessible names). The default window's split
  `calendar-panes` lets its panes shrink to their `min-sizes`; a rule for every split belongs in the
  layout renderer (`flexForSize` gives fixed sizes `flex-shrink: 0`) and is requested from the
  integrator. No `:has()` with a descendant search: it is costly in jsdom's style matching (the axe
  tests run it for every element); the PIM axe file ran slower with it.
- **Events:** the title is clamped (3 lines, 1 line in an event shorter than 3.5rem through a
  `nail-event` size container), time and place never shrink. All-day chips drop their "All day".
- **Responsive rules select through two parts** (e.g. `contact-list > contact-list.toolbar >
  form.field`), so they win over the single-part rules still in `defaults/theme.kdl` without
  editing it; `PIM_SKIN_PARTS` (the clash list) leaves that section out.
- **Reading width:** a contact card's content and the task list stop at 56rem in a wide window.
- **Customize-layout mode:** the canvas scrolls sideways (`min-width: min-content` on the window it
  shows) instead of cutting the last pane; the banner's hint takes a wide line and the buttons wrap
  below it.
- **Toolbar customizer:** its narrow layout is a container query on the customizer
  (`nail-toolbar-customizer`, 40rem), the lists shrink only to a few rows and the body scrolls
  below that; italic labels get `padding-inline-end: 0.2em` for the glyphs' overhang.
- **Theme editor:** one grid per token row — name, origin tag, controls at a fixed 60 % share — so
  every tag ends at the same edge and long values fit; below 40rem of editor width the controls go
  under the name; the preview column moves below the panel under 64rem; a preview narrower than
  22rem leaves out its folder column. In the window the editor has no heading of its own (the title
  bar names it; the region keeps the name through `aria-label`); placed as a layout widget it keeps
  its heading.
- **Appearance:** the light / dark theme fields form one pair without the settings rows' separator,
  label and select on one line.
- **Duplicated built-in themes** get their own header comment (`// <name>: a copy of the built-in
  theme <source>, made in Nail.` plus where to edit it); a duplicated user theme keeps the user's
  comments. English, like the other headers Nail writes into config files.
- **Windows that stay open while the viewport shrinks** keep their place partly off the page
  (`keepOnPage`); that is the window manager's (WP191) and is reported, not changed here. The spec
  opens each window afresh per size.

## Consequences

- `e2e/tests/p1-views.spec.ts` sweeps every calendar view, contacts, tasks, customize-layout mode,
  the toolbar customizer, Appearance and the theme editor (both tabs) at the 8 standard sizes, the
  two editor windows at min-size and 1.5 × their size, and checks the cosmetic fixes (one title, the
  Separator's room, tags in one column on one line).
- Visual baselines of the calendar, Appearance, theme editor, layout editor and toolbar customizer
  screens change; the integrator regenerates them.
