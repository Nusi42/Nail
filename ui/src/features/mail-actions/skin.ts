/**
 * The look of the undo toast (P3 WP-actions-ui, ADR 0164) on the token
 * schema only (`var(--nail-…)`), like `../../theme/skin.ts`: a dark-on-light
 * (inverted) bar at the bottom centre of the window, Gmail style, above
 * every window (it is a popover in the top layer). The compiler emits it
 * after the shared skin, so any `theme.kdl` part rule still wins.
 *
 * Parts: `mail-actions.toast` (`data-kind` = `done` / `undone` / `error`,
 * `data-action` = the `MailActionKind`), `mail-actions.toast.text`,
 * `mail-actions.toast.undo`, `mail-actions.toast.dismiss` (+ `.icon`).
 */
import { p } from "../../theme/selectors";
import { CAPS_TIGHT, floatingWidth } from "../../theme/declarations";
import { T } from "../../theme/tokens";

export const MAIL_ACTIONS_SKIN_CSS = `${p("mail-actions.toast")} {
  position: fixed; inset: auto auto 24px 50%; transform: translateX(-50%); margin: 0;
  display: flex; align-items: center; gap: ${T.sm};
  max-width: ${floatingWidth("560px")}; padding: ${T.xs} ${T.xs} ${T.xs} ${T.md};
  border: 0; border-radius: ${T.rMd}; box-shadow: ${T.overlay}; overflow: visible;
  font-family: ${T.fontUi}; font-size: ${T.body}; color: ${T.bg}; background-color: ${T.fg};
}
${p("mail-actions.toast")}[data-kind="error"] { color: ${T.bg}; background-color: ${T.danger}; }
${p("mail-actions.toast.text")} { flex: 1 1 auto; min-width: 0; overflow-wrap: anywhere; }
${p("mail-actions.toast.undo")}, ${p("mail-actions.toast.dismiss")} {
  display: inline-flex; align-items: center; justify-content: center; min-height: ${T.control};
  padding: 0 ${T.sm}; border: 0; border-radius: ${T.rMd}; color: inherit; background: transparent;
  font: inherit; cursor: pointer;
}
${p("mail-actions.toast.undo")} { font-weight: ${T.semibold}; ${CAPS_TIGHT} }
${p("mail-actions.toast.dismiss")} { min-width: ${T.control}; padding: 0; }
${p("mail-actions.toast.undo")}:hover, ${p("mail-actions.toast.dismiss")}:hover { background-color: color-mix(in srgb, currentColor 16%, transparent); }
${p("mail-actions.toast.undo")}:focus-visible, ${p("mail-actions.toast.dismiss")}:focus-visible { outline: 2px solid ${T.focus}; outline-offset: 1px; }
`;
