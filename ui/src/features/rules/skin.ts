/**
 * The look of the rules window's R1 pieces on the token schema only
 * (`var(--nail-…)`), like `../../theme/skin.ts`; the theme compiler emits it
 * after the shared skin, so any `theme.kdl` part rule still wins.
 *
 * - `rules.issues`: the compact problem indicator (`./Issues.tsx`) — a small
 *   outlined badge in danger (`data-kind="error"`) or warning colour that
 *   never changes the height of its row; its popover `rules.issues.details`
 *   is a floating card in the top layer (it never moves the content).
 * - `rules.run-panel`: the run panel under the rule list (`./RunPanel.tsx`);
 *   its outcome line `rules.run.outcome` keeps one line of room, so a result
 *   arriving moves nothing.
 */
import { p } from "../../theme/selectors";
import { floatingWidth, LINE_BOX } from "../../theme/declarations";
import { T } from "../../theme/tokens";

/** The widest an indicator's explanation gets (a few lines of errors). */
const DETAILS_WIDTH = "36em";

export const RULES_SKIN_CSS = `${p("rules.views")} { align-items: center; flex-wrap: wrap; }
${p("rules.issues")} { display: inline-flex; align-items: center; }
${p("rules.issues.badge")} {
  display: inline-flex; align-items: center; gap: ${T.xxs}; padding: 0 ${T.xs};
  border: 1px solid currentColor; border-radius: ${T.pill}; font: inherit; font-size: ${T.small};
  line-height: ${T.tight}; color: ${T.danger}; background: transparent; cursor: help;
}
${p("rules.issues.badge")}[data-kind="warning"] { color: ${T.warning}; }
${p("rules.issues.badge")}:focus-visible { outline: 2px solid ${T.focus}; outline-offset: 1px; }
${p("rules.issues.details")} {
  max-width: ${floatingWidth(DETAILS_WIDTH)}; padding: ${T.sm} ${T.md};
  border: 1px solid ${T.border}; border-radius: ${T.rMd}; box-shadow: ${T.overlay};
  color: ${T.fg}; background-color: ${T.bg}; font-family: ${T.fontUi}; font-size: ${T.body};
}
${p("rules.issues.items")} { margin: 0; padding-left: ${T.lg}; }
${p("rules.issues.item")} { overflow-wrap: anywhere; }
${p("rules.run-panel")} {
  display: flex; flex-direction: column; gap: ${T.sm}; padding-top: ${T.sm};
  border-top: 1px solid ${T.border};
}
${p("rules.run.outcome")} { min-height: ${LINE_BOX}; margin: 0; font-size: ${T.small}; color: ${T.muted}; }
`;
