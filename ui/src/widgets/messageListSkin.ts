/**
 * Conversation rows of the message list (P2, ADR 0148), on the token schema
 * only (`var(--nail-…)`), like `../theme/skin.ts` (which keeps the M5a row
 * look: avatar, lines, tag line). The compiler emits this after the shared
 * skin and before the theme's part rules (`../theme/compile.ts`), so a
 * `theme.kdl` still overrides any rule here.
 *
 * Contract / quick win: the count pill (`message-list.row.count`, the
 * template field `{count | if-many}`, empty and hidden for a single
 * message) and the people of a conversation (`message-list.row.participants`
 * with `.participant` names, `me` / `unread` states, `.separator`). WP-list
 * (P2) owns this file: Design A's rows and Design B's expander and child
 * rows (`message-list.row.expander`, `message-list.child*`).
 */

/** `[data-part="…"]` for one part. */
const p = (part: string) => `[data-part="${part}"]`;

const T = {
  muted: "var(--nail-color-fg-muted)",
  border: "var(--nail-color-border)",
  small: "var(--nail-font-size-small)",
  semibold: "var(--nail-font-weight-semibold)",
  xs: "var(--nail-space-xs)",
  lg: "var(--nail-space-lg)",
  pill: "var(--nail-radius-pill)",
} as const;

const ROW = p("message-list.row");

export const MESSAGE_LIST_SKIN_CSS = `${p("message-list.row.count")} {
  flex: 0 0 auto; box-sizing: border-box; min-width: calc(${T.lg} + ${T.xs}); padding: 0 ${T.xs};
  border: 1px solid ${T.border}; border-radius: ${T.pill};
  font-size: ${T.small}; font-weight: ${T.semibold}; line-height: 1.4; text-align: center; font-variant-numeric: tabular-nums;
  color: ${T.muted};
}
${p("message-list.row.count")}:empty { display: none; }
${ROW}[data-state~="selected"] ${p("message-list.row.count")} { color: inherit; border-color: currentColor; }
${p("message-list.row.participants")} { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
${p("message-list.row.participant")}[data-state~="unread"] { font-weight: ${T.semibold}; }`;
