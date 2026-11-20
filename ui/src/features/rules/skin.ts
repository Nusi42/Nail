/**
 * The look of the rules window's R1 pieces on the token schema only
 * (`var(--nail-…)`), like `../../theme/skin.ts`; the theme compiler emits it
 * after the shared skin, so any `theme.kdl` part rule still wins.
 *
 * - `rules.issues.items`: the list of problems inside the shared problem
 *   indicator's details (`./Issues.tsx`; the indicator's own look is the
 *   shared skin's `PROBLEM`, ADR 0221).
 * - `rules.run-panel`: the run panel under the editor's buttons
 *   (`./RunPanel.tsx`); its outcome line `rules.run.outcome` keeps one line
 *   of room, so a result arriving moves nothing.
 * - `rules.notice` in the view switch's row takes no more height than the
 *   switch, so "The rule is saved." moves nothing either.
 * - `rules.list.head`: the list's column names (`theme.kdl` makes the list a
 *   grid whose rows share its columns).
 */
import { p } from "../../theme/selectors";
import { LINE_BOX } from "../../theme/declarations";
import { T } from "../../theme/tokens";

export const RULES_SKIN_CSS = `${p("rules.views")} { align-items: center; flex-wrap: wrap; }
${p("rules.issues.items")} { margin: 0; padding-left: ${T.lg}; }
${p("rules.issues.item")} { overflow-wrap: anywhere; }
${p("rules.run-panel")} {
  display: flex; flex-direction: column; gap: ${T.sm}; padding-top: ${T.sm};
  border-top: 1px solid ${T.border};
}
${p("rules.views")} ${p("rules.notice")} { padding-block: 0; line-height: ${LINE_BOX}; }
${p("rules.list.head.cell")} { font-size: ${T.small}; color: ${T.muted}; }
${p("rules.run.outcome")} { min-height: ${LINE_BOX}; margin: 0; font-size: ${T.small}; color: ${T.muted}; }
`;
