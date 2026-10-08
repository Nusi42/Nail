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
 * with `.participant` names, `me` / `unread` states, `.separator`).
 *
 * WP202 (P2):
 * - Design A: "Sender:" of the third line (`message-list.row.latest-from`,
 *   `{latest_from | names(1) | then(":")}`) beside the muted preview; in
 *   Compact the preview line (`data-fields~="preview"`) is hidden, so a row
 *   is two lines.
 * - Design B (`threads="expandable"`): the chevron in a third column
 *   (`message-list.row.expander`, turned down while open, invisible for a
 *   single message), child rows (`message-list.child`) one line each with
 *   a rail (`.rail`) under the conversation's avatar, the sender (`.from`,
 *   bold while unread), the preview (`.preview`) and the date (`.date`);
 *   selected children take the selection colours like rows. Child rows
 *   use body-size text and padding from `space.row` (about 40 px tall in
 *   Comfortable, 32 px in Compact).
 * Colours: only `fg` / `fg-muted` / `border` on `bg` / `bg-subtle` and
 * `selection-fg` on `selection` — pairs `tests/theme_contrast.rs` checks.
 */
import { HC, p } from "../theme/selectors";
import { HC_RING } from "../theme/declarations";
import { T } from "../theme/tokens";

const ROW = p("message-list.row");
const CHILD = p("message-list.child");
/**
 * A child row's padding: five sixths of the list's row padding, so a child
 * row is about 40 px tall in Comfortable and 32 px in Compact (one line of
 * body text; the proposal's density table), never below the pointer minimum.
 */
const CHILD_PAD = `calc(${T.row} * 5 / 6)`;
const EXPANDER = p("message-list.row.expander");
const COMPACT = [':root[data-nail-density="compact"]', `${p("app")}[data-density="compact"]`];

export const MESSAGE_LIST_SKIN_CSS = `${p("message-list.row.count")} {
  flex: 0 0 auto; box-sizing: border-box; min-width: calc(${T.lg} + ${T.xs}); padding: 0 ${T.xs};
  border: 1px solid ${T.border}; border-radius: ${T.pill};
  font-size: ${T.small}; font-weight: ${T.semibold}; line-height: 1.4; text-align: center; font-variant-numeric: tabular-nums;
  color: ${T.muted};
}
${p("message-list.row.count")}:empty { display: none; }
${ROW}[data-state~="selected"] ${p("message-list.row.count")} { color: inherit; border-color: currentColor; }
${p("message-list.row.participants")}, ${p("message-list.row.latest-from")} { display: flex; min-width: 0; overflow: hidden; white-space: nowrap; }
${p("message-list.row.participant-group")} { display: flex; flex: 0 1 auto; min-width: 0; }
/* "me" never shrinks when the names do not fit (P2 integration). */
${p("message-list.row.participant-group")}[data-state~="me"] { flex-shrink: 0; }
${p("message-list.row.separator")}, ${p("message-list.row.suffix")} { flex: none; white-space: pre; }
${p("message-list.row.participant")} { flex: 0 1 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
${p("message-list.row.participant")}[data-state~="unread"] { font-weight: ${T.semibold}; }
${p("message-list.row.latest-from")} { flex: 0 1 auto; max-width: 45%; font-size: ${T.small}; }
${p("message-list.row.latest-from")} ${p("message-list.row.participant")}[data-state~="unread"] { font-weight: inherit; }
${p("message-list.row.latest-from")}:empty { display: none; }
${p("message-list.row.line")}[data-fields~="preview"] ${p("message-list.row.preview")} { flex: 1 1 0; min-width: 0; }
${COMPACT.map((c) => `${c} ${p("message-list.row.line")}[data-fields~="preview"]`).join(", ")} { display: none; }
${ROW}:has(> ${EXPANDER}) { grid-template-columns: ${T.avatar} minmax(0, 1fr) auto; }
${EXPANDER} {
  display: grid; place-items: center; align-self: center; box-sizing: border-box;
  width: calc(${T.lg} + ${T.xs}); height: calc(${T.lg} + ${T.xs}); margin-inline-start: ${T.xs};
  border-radius: ${T.rSm}; color: ${T.muted}; cursor: pointer;
}
${EXPANDER}:hover { color: ${T.fg}; background-color: ${T.subtle}; }
${EXPANDER}[data-state~="single"] { visibility: hidden; cursor: default; }
${p("message-list.row.expander.icon")} { width: ${T.lg}; height: ${T.lg}; transition: transform 120ms ease; }
${EXPANDER}[data-state~="expanded"] ${p("message-list.row.expander.icon")} { transform: rotate(90deg); }
${ROW}[data-state~="selected"] ${EXPANDER} { color: inherit; }
${ROW}[data-state~="selected"] ${EXPANDER}:hover { background-color: transparent; }
${ROW}[data-state~="expanded"] { border-bottom-color: transparent; }
${CHILD} {
  position: relative; display: grid; grid-template-columns: ${T.avatar} minmax(0, max-content) minmax(0, 1fr) auto;
  align-items: baseline; column-gap: ${T.md};
  padding: ${CHILD_PAD} ${T.pane}; border-bottom: 1px solid transparent;
  font-size: ${T.body}; line-height: ${T.normal}; color: ${T.fg}; background-color: ${T.bg}; cursor: default;
}
${CHILD}:has(+ :not(${CHILD})) { border-bottom-color: ${T.border}; }
${CHILD}:hover { background-color: ${T.subtle}; }
${CHILD}[data-state~="selected"] { color: ${T.selectionFg}; background-color: ${T.selection}; }
${p("message-list")}:focus-visible ${CHILD}[aria-selected="true"] { outline: 2px solid ${T.focus}; outline-offset: -2px; }
${HC} ${p("message-list")}:focus-visible ${CHILD}[aria-selected="true"] { outline: ${HC_RING} solid ${T.focus}; }
${p("message-list.child.rail")} {
  justify-self: center; align-self: stretch; width: 2px; margin-block: calc(-1 * ${CHILD_PAD});
  border-radius: 1px; background-color: ${T.border};
}
${CHILD}[data-state~="selected"] ${p("message-list.child.rail")} { background-color: currentColor; }
${p("message-list.child.from")}, ${p("message-list.child.preview")} { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
${p("message-list.child.from")} { max-width: calc(12 * ${T.lg}); }
${CHILD}[data-state~="unread"] ${p("message-list.child.from")} { font-weight: ${T.semibold}; }
${p("message-list.child.preview")} { color: ${T.muted}; }
${p("message-list.child.date")} { font-size: ${T.small}; white-space: nowrap; font-variant-numeric: tabular-nums; color: ${T.muted}; }
${CHILD}[data-state~="selected"] ${p("message-list.child.preview")}, ${CHILD}[data-state~="selected"] ${p("message-list.child.date")} { color: inherit; }`;
