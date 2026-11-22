/**
 * The structure of the shared dialog pattern (R6, ADR 0239), on the token
 * schema only (`var(--nail-…)`), like `../../theme/skin.ts`. The compiler
 * emits it after the shared skin and before the responsive rules and the
 * theme's part rules, so `theme.kdl` (`dialog.*`, `settings-save-bar`)
 * still overrides it.
 *
 * - `dialog.confirm`: the question, its detail, then the footer at the
 *   bottom of the window.
 * - `dialog.footer`: one row — `dialog.footer.start` (an editor's
 *   `Delete…`) at the start, `dialog.footer.end` (the buttons by role) at
 *   the end. As the last child in a window body it sticks to the bottom
 *   while the body scrolls (`DIALOG_BUTTON_ROWS`, `../../theme/skinResponsive.ts`).
 * - `settings-save-bar`: pinned below the settings section's body (the
 *   section content is a column; the body scrolls above it).
 */
import { p } from "../../theme/selectors";
import { T } from "../../theme/tokens";

export const DIALOG_SKIN_CSS = `${p("dialog.confirm")} { display: flex; flex-direction: column; gap: ${T.md}; min-height: 100%; }
${p("dialog.question")} { margin: 0; overflow-wrap: anywhere; }
${p("dialog.detail")} { display: grid; gap: ${T.sm}; }
${p("dialog.note")} { margin: 0; color: ${T.muted}; overflow-wrap: anywhere; }
${p("dialog.footer")} { display: flex; flex-wrap: wrap; align-items: center; gap: ${T.sm}; margin-block-start: auto; }
${p("dialog.footer.start")} { display: flex; flex-wrap: wrap; gap: ${T.sm}; margin-inline-end: auto; }
${p("dialog.footer.end")} { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: ${T.sm}; margin-inline-start: auto; }
${p("settings-save-bar")} {
  display: flex; flex-wrap: wrap; align-items: center; gap: ${T.sm} ${T.md}; flex: 0 0 auto;
  padding: ${T.sm} ${T.xxl}; border-top: 1px solid ${T.border}; background-color: ${T.subtle};
}
${p("settings-save-bar.text")} { margin: 0; font-weight: ${T.medium}; overflow-wrap: anywhere; }
${p("settings-save-bar")} > ${p("dialog.footer")} { margin-inline-start: auto; }
${p("window.body")} ${p("settings-save-bar")} > ${p("dialog.footer")}:last-child {
  position: static; padding: 0; border: none; background: none;
}`;
