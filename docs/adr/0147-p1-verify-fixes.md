# ADR 0147: P1 verify fixes — the Appearance pair, text running out of its box, the composer's address fields

Status: accepted · 2026-10-05 · P1 verify (fixes ADRs 0139, 0142, 0145)

## Context

P1's verify gate failed on one blocking finding and two smaller ones:

- Settings → Appearance at about 1024–1150 px cut the "Light theme" / "Dark theme" labels to "Light t" / "Dark th".
  ADR 0145 had put `white-space: nowrap` on the labels, but the settings card's row grid gives the label column
  `minmax(0, 1fr)` beside a 16-unit control column: in a pair field of about 300 px the label column shrank to
  26 px, and the one-line text ran under the select, which painted over it.
- The responsive helper (`e2e/support/responsive.ts`, ADR 0139) did not see it: the label's box was not clipped
  (overflow visible) and did not overlap the select — only its text did.
- In the composer's security row "Technology" was a bold field label beside the regular "Sign" / "Encrypt" checks.
- The final screenshots for the maintainer's review showed the composer's To field cut after about 20 characters.

## Decisions

- **The pair gives its labels their own width.** Where a card puts labels beside their controls (the `nail-fields`
  container wider than `FIELDS_NARROW`), each field of `appearance.pair` is `max-content minmax(0, 1fr)`: the label
  on one line at its own width, the select taking the rest. The pair puts its two fields side by side only from
  18 spacing units each (`theme/skin.ts`), so the select keeps a usable width; below that the fields stack, and a
  narrow card puts every label above its control, the pair's too. `white-space: nowrap` lives only inside that
  container query.
- **The helper sees text that runs out of its box.** A new kind `overflowing`: an element whose text (its own text
  nodes and those of plain descendants, not of a descendant `[data-part]`; each cut by its clipping ancestors)
  sticks out of its box sideways while the box does not clip it (overflow visible, no ellipsis). The `overlap`
  check counts a text leaf together with its sideways spill, so a label running under its neighbour is an overlap.
  Only sideways: a line's glyph box is a little taller than the line, and stacked lines would "overlap" by a pixel
  or two. Self-tests: a nowrap label in a narrow box, the "Light theme" case (label + select in a grid), text a
  scrolling pane hides. Rerunning `p1-mail`, `p1-windows` and `p1-views` with the extended helper found only the
  Appearance pair.
- **Address fields span their row.** Found in the maintainer's screenshot review: the composer's To / Cc / Bcc input sits
  inside the suggestion anchor `composer.address`, a plain block, so it kept the browser's default size (about 20
  characters) and cut "Ben Ortiz <ben@nail.example>" while Subject spanned the row. The input now fills the anchor
  (`width: 100%`), the anchor may shrink (`min-width: 0`); `p1-windows.spec.ts` checks that To and Subject end at
  the same edge. An input scrolls its own text, so the helper cannot see this; the screenshot review did.
- **"Technology" reads like its neighbours.** `security-compose.toggles > form.field > form.label` takes the body
  size and the regular weight, like the check labels beside it.

## Consequences

- `p1-views.spec.ts` checks the pair at 1024, 1080, 1150 and 1200 px: each label's text ends inside its box and
  before its select, and the helper finds nothing in the pair.
- Visual baselines changed only for the `composer` screen of the four comfortable non-high-contrast themes (the
  Technology label; the attachment reminder above the footer moves by a pixel or two with the row's height). The
  `appearance` screen is captured wide enough that the pair looked the same before.
