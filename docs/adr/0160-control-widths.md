# ADR 0160: Controls fill their column; small ones say so (`data-width`)

Status: accepted · 2026-10-05 · maintainer requirement (side package beside P2; amends ADRs 0105, 0139, 0147)

Numbers 0160–0162 were taken for this side package so they stay clear of the parallel P2 wave (ADR 0148 and its
package ADRs).

## Context

Review finding: in the composer the To field was too short, and the same fault appeared throughout the app.
P1 fixed the composer's To field (ADR 0147), but the class of bug was systemic: an `<input>`,
`<select>` or `<textarea>` is an inline-block whose width without a rule is the browser's intrinsic size (about
20 characters, 150–250 px). Every row, card or dialog that did not happen to stretch its controls left them short
and out of line with their siblings. The responsive helper (ADR 0139) checked clipping, overlaps and alignment of
labels, not this.

## Decisions

- **The helper finds it.** `e2e/support/responsive.ts` has a new issue kind `undersized-control`, checked
  everywhere the helper runs (`layoutIssues`, `expectResponsive`, the `p1-windows` / `p1-views` / `p1-mail` sweeps).
  Candidates are text-like controls: inputs other than checkbox / radio / range / color / file / buttons, textareas,
  selects, combo boxes, contenteditable editors and date / time fields drawn as segments (a `role="group"` of
  spinbuttons); hidden form proxies (inside `aria-hidden="true"`) are not. A control is undersized when
  1. walking up from it past every box whose width comes from its content (inline-level, a flex item that does not
     grow, a flex-column item that is not stretched, a grid item that is not stretched), the first container that
     sets the width offers at least 48 px of visually free room beside it on its line (no control, text or picture
     there) and the control uses less than 85 % of the width it could have — a control stretched by its grid area
     or growing in its flex row is never undersized; or
  2. it stops at least 24 px short of the right edge that at least two other controls starting at its left edge share
     (its form column), with nothing on its line in between.
  Layout boxes are used, not visible ones: a field scrolled out of its pane still has its row.
  Self-tests in `e2e/tests/responsive-helper.spec.ts` (found: To in a block wrapper, a flex row, a start-aligned grid,
  a textarea in a block; not found: filling, growing, marked controls, a date beside a time, an editor; the column
  rule).
- **Marking instead of whitelisting.** A control that is small on purpose carries `data-width="short" | "medium" |
  "content"` on itself or on its picker's root; the helper skips it and the shared skin sizes it
  (`theme/skin.ts` `CONTROL_WIDTHS`: `short` = 7 spacing units for a port, minutes, a count, a time of day;
  `medium` = 16 units, the settings cards' control column, for a filter or chooser in a header; `content` = its own
  width, for a view switcher or an option among toggles). The width travels as `--nail-skin-control-width`, so the
  settings cards' control column honours it too. `SelectField` has a `width` prop, `NumberField` and every number /
  time input are `short`.
- **Fill by default.** `form.input`, `form.select` and `form.textarea` get `width: var(--nail-skin-control-width,
  100%)`, `min-width: 0`, `max-width: 100%` and `flex-basis: content`: in a block, a grid or a flex column they fill
  their field's column; in a flex row their basis stays their content, so a wrapping row does not put each control
  on its own line and a column flex box does not grow them taller (`flex-grow` or a basis would act on the height
  there).
- **Rows of controls are named once.** Rows where controls stand side by side and should share the row are listed in
  `theme/skinResponsive.ts` `FIELD_ROWS` (`rules.add`, `rules.condition`, `rules.action`, `toolbar-editor.add`,
  `column-editor.add`, `vacation-editor.dates`, `appearance.accent`): each field or control there is `flex: 1 1 8rem`
  (wrapping below that), buttons keep their width, and the row aligns its items at the bottom so a button lines up
  with the control under a field's label. The event editor's date field grows beside its "Choose" button and the
  short time field (`features/pim/skin.ts`).
- **The sweeps reach the forms.** `p1-windows.spec.ts` checks the composer with Cc and Bcc, the rule editor with
  conditions and actions, the account setup's server step, and a new `pim` group with every PIM dialog (event editor,
  meeting, edit scope, task / contact editor, import, sources, groups, duplicates — before, `OTHER_PACKAGES` named them
  but no spec opened them). `expectLinedUp` asserts that To / Cc / Bcc / Subject, the account setup's Server / User
  name, the event editor's Title / Location / Description and the contact editor's First name / Last name / Shown as
  share both edges at 1600, 1024 and 800 px, and that the rule editor's condition and "add" rows span its Match select.

## Findings and fixes

| Place | Before | Fixed |
| --- | --- | --- |
| Composer, security row "Technology" | 102 px select, up to 1,224 px free | `width="content"` (an option among the toggles) |
| Composer To / Cc / Bcc / Subject | To fixed in ADR 0147; not asserted with Cc / Bcc | lined up at three widths |
| Rule editor, "Condition" / "Action" chooser of the add row | 251 / 207 px, 171–683 px free | `FIELD_ROWS`: fills beside its button |
| Rule editor, a condition's / action's selects and text | intrinsic, row half empty (not swept before) | `FIELD_ROWS`: the row's controls share it |
| Settings → Server rules & vacation, First day / Last day | 134–256 px date inputs, 135–472 px free | `FIELD_ROWS`: each date takes half the row |
| Settings → Appearance, accent | 165 px text input, 250–633 px free | `FIELD_ROWS`: fills beside the colour picker |
| Settings → Layout & toolbars, column / action chooser | 89 / 273 px, up to 657 px free | fill beside their buttons (`FIELD_ROWS`) |
| Settings → Remote access, audit user filter | 90–256 px | `width="medium"` |
| Toolbar customizer, "Toolbar" chooser at min-size | 141 px, 305 px free | `data-width="medium"` |
| Calendar toolbar (narrow), view menu | 96 px | `data-width="content"` (a view switcher) |
| Event editor, Start / End date | 90 px segment field, about 470 px free | grows to the row's edge |
| Event editor, Start / End time | intrinsic segment field | `data-width="short"` |
| Number / time inputs (ports, minutes, intervals, quiet hours, …) | intrinsic | `data-width="short"` (they would otherwise fill) |
| Export / Backup wizard path field | reported against its column (the helper measured a scrolled-out row) | helper uses layout boxes; nothing to fix |
| Event editor free/busy hour labels at min-size (found by the new `pim` group) | cut off ("clipped") | end in an ellipsis |

## Consequences

- A new control fills its column without a rule; one that should be small says so with `data-width`, and a new row of
  controls side by side goes into `FIELD_ROWS`. A control left at the browser's width fails the P1 sweeps.
- Visual baselines change wherever a control used to be intrinsic (listed in the commit).
- The check needs layout, so it runs only in Playwright; Vitest checks the skin rules (`skin.test.tsx`,
  `skinResponsive.test.ts`).
