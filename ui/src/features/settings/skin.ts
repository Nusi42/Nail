/**
 * The M5b look of the settings page (navigation, search, sections) (WP163,
 * ADR 0109), on the token schema only (`var(--nail-…)`), like
 * `../../theme/skin.ts`. The compiler emits it right after the shared skin
 * and before the theme's part rules, so any `theme.kdl` still overrides it.
 *
 * - The page: navigation (`settings-page.nav`, search, groups, results) on
 *   the left, the shown section (`settings-page.content`) on the right; one
 *   column below 720 px.
 * - Each widget of a section sits on a card (`settings-page.card`); inside a
 *   card, form fields become rows — label and description on the left, the
 *   control on the right — separated by hairlines, and the widgets' own
 *   cards and sections lose their borders so cards do not nest.
 * - P1 (WP191): the card is the container its rows measure: every control
 *   of a card shares one column (one left edge, one width); in a narrow card
 *   the label goes above its control. A widget's sections without a layout
 *   of their own stack their rows with a gap (checks one per row, not glued
 *   inline), and the page pads less when it is narrow.
 * - Search matches are `settings-page.mark`; a revealed setting carries
 *   `data-settings-found` for a moment.
 * - M5a review points: a segmented control (Density) keeps its natural width
 *   instead of stretching across its card, and its selected segment is
 *   filled in High contrast (not only bold).
 */

import { FIELDS_CONTAINER } from "../../theme/skinResponsive";
import { CONTROL_WIDTH_VAR } from "../../theme/skin";
import { p } from "../../theme/selectors";
import { QUIET_BUTTON, QUIET_HOVER, SECTION_LABEL } from "../../theme/declarations";
import { T } from "../../theme/tokens";

/** A part inside the settings page's cards. */
const inCard = (part: string) => `${p("settings-page.card")} ${p(part)}`;

const focusRing = `outline: 2px solid ${T.focus}; outline-offset: -2px;`;

const PAGE = `${p("settings-page")} {
  display: grid; grid-template-columns: minmax(calc(13 * ${T.lg}), calc(16 * ${T.lg})) minmax(0, 1fr);
  height: 100%; min-height: 0; overflow: hidden; color: ${T.fg}; background-color: ${T.bg}; font-family: ${T.fontUi};
}
${p("settings-page")}[data-state~="empty"] { display: block; padding: ${T.xl}; }
${p("settings-page.empty")} { margin: 0; color: ${T.muted}; }
${p("settings-page.nav")} {
  display: grid; align-content: start; gap: ${T.xs}; min-height: 0; overflow: auto;
  padding: ${T.md} ${T.sm}; border-inline-end: 1px solid ${T.border}; background-color: ${T.subtle};
}
${p("settings-page.content")} { display: flex; flex-direction: column; min-width: 0; min-height: 0; overflow: hidden; }
${p("settings-page.header")} { display: grid; gap: ${T.xxs}; padding: ${T.xl} ${T.xxl} ${T.md}; }
${p("settings-page.breadcrumb")} { ${SECTION_LABEL} }
${p("settings-page.title")} { margin: 0; font-family: ${T.fontDisplay}; font-size: ${T.title}; font-weight: ${T.semibold}; line-height: ${T.tight}; }
${p("settings-page.title")}:focus { outline: none; }
${p("settings-page.title")}:focus-visible { outline: 2px solid ${T.focus}; outline-offset: 2px; }
${p("settings-page.body")} {
  display: grid; align-content: start; gap: ${T.lg}; flex: 1 1 auto; min-height: 0; overflow: auto;
  padding: ${T.xs} ${T.xxl} ${T.xxl};
}
${p("settings-page.body")} > * { max-width: calc(52 * ${T.lg}); }
@media (max-width: 720px) {
  ${p("settings-page")} { grid-template-columns: minmax(0, 1fr); grid-template-rows: auto minmax(0, 1fr); }
  ${p("settings-page.nav")} { max-height: 40vh; border-inline-end: none; border-bottom: 1px solid ${T.border}; }
  ${p("settings-page.header")} { padding: ${T.lg} ${T.pane} ${T.sm}; }
  ${p("settings-page.body")} { padding: ${T.xs} ${T.pane} ${T.xl}; }
}`;

const NAV = `${p("settings-page.search")} {
  display: flex; align-items: center; gap: ${T.sm}; min-height: ${T.control}; margin: 0 ${T.xxs} ${T.sm}; padding: 0 ${T.sm};
  border: ${T.controlBorder}; border-radius: ${T.rMd}; color: ${T.muted}; background-color: ${T.bg};
}
${p("settings-page.search")}:focus-within { outline: 2px solid ${T.focus}; outline-offset: 0; border-color: ${T.accent}; }
${p("settings-page.search.icon")} { flex: 0 0 auto; width: ${T.icon}; height: ${T.icon}; }
${p("settings-page.search.input")} {
  flex: 1 1 auto; min-width: 0; min-height: calc(${T.control} - 2px); padding: 0; border: none; outline: none;
  font-family: ${T.fontUi}; font-size: ${T.body}; color: ${T.fg}; background: transparent;
}
${p("settings-page.search.input")}::-webkit-search-cancel-button { display: none; }
${p("settings-page.search.clear")} {
  display: inline-flex; align-items: center; justify-content: center; flex: 0 0 auto;
  width: calc(${T.control} - ${T.sm}); height: calc(${T.control} - ${T.sm}); padding: 0;
  ${QUIET_BUTTON}
}
${p("settings-page.search.clear")}:hover { ${QUIET_HOVER} }
${p("settings-page.search.clear")}:focus-visible { ${focusRing} }
${p("settings-page.search.clear.icon")} { width: ${T.icon}; height: ${T.icon}; }
${p("settings-page.group")} { display: grid; gap: ${T.xxs}; }
${p("settings-page.group.label")} {
  padding: ${T.md} ${T.md} ${T.xs}; ${SECTION_LABEL}
}
${p("settings-page.group.list")}, ${p("settings-page.results")}, ${p("settings-page.result.settings")} {
  display: grid; gap: ${T.xxs}; margin: 0; padding: 0; list-style: none;
}
${p("settings-page.nav.item")}, ${p("settings-page.result.section")}, ${p("settings-page.result.setting")} {
  display: flex; align-items: center; gap: ${T.md}; width: 100%; min-height: ${T.control}; padding: 0 ${T.md};
  border: none; border-radius: ${T.rSm}; font-family: ${T.fontUi}; font-size: ${T.body}; text-align: start;
  color: ${T.fg}; background-color: transparent; transition: background-color ${T.fast} ${T.ease};
}
${p("settings-page.nav.item")}:hover, ${p("settings-page.result.section")}:hover, ${p("settings-page.result.setting")}:hover { background-color: ${T.raised}; }
${p("settings-page.nav.item")}:focus-visible, ${p("settings-page.result.section")}:focus-visible, ${p("settings-page.result.setting")}:focus-visible { ${focusRing} }
${p("settings-page.nav.item")}[aria-current="page"], ${p("settings-page.result.section")}[aria-current="page"] {
  color: ${T.selectionFg}; background-color: ${T.selection}; font-weight: ${T.semibold};
}
${p("settings-page.nav.icon")} { flex: 0 0 auto; width: ${T.icon}; height: ${T.icon}; color: ${T.muted}; }
${p("settings-page.nav.item")}[aria-current="page"] ${p("settings-page.nav.icon")}, ${p("settings-page.result.section")}[aria-current="page"] ${p("settings-page.nav.icon")} { color: inherit; }
${p("settings-page.nav.label")} { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
${p("settings-page.result.setting")} {
  min-height: calc(${T.control} - ${T.xs}); padding-inline-start: calc(${T.icon} + 2 * ${T.md});
  font-size: ${T.small}; color: ${T.muted};
}
${p("settings-page.result.label")} { min-width: 0; overflow-wrap: anywhere; }
${p("settings-page.result-count")} { margin: 0; padding: ${T.xs} ${T.md}; font-size: ${T.small}; color: ${T.muted}; }
${p("settings-page.mark")} {
  padding: 0; border-radius: 2px; font-weight: ${T.semibold}; color: inherit;
  background-color: color-mix(in srgb, ${T.accent} 22%, transparent);
  box-shadow: 0 1px 0 0 ${T.accent};
}`;

/** Below this container width a form row puts its label above the control. */
export const FIELDS_NARROW = "30rem";
/** The width of the control column of a form row (P1: one left edge per card). */
const CONTROL_COLUMN = `calc(16 * ${T.lg})`;

const CARDS = `${p("settings-page.card")} {
  display: grid; gap: ${T.md}; min-width: 0; padding: ${T.lg};
  border: 1px solid ${T.border}; border-radius: ${T.rLg}; background-color: ${T.bg};
  container: ${FIELDS_CONTAINER} / inline-size;
}
${p("settings-page.card")}:focus { outline: none; }
${p("settings-page.card")} > [data-part] { min-width: 0; padding: 0; overflow: visible; }
${inCard("form.field")} {
  display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, ${CONTROL_COLUMN}); align-items: center;
  column-gap: ${T.lg}; row-gap: ${T.xxs}; padding-block: ${T.sm};
}
${inCard("form.field")} > * { grid-column: 1 / -1; }
${inCard("form.field")} > ${p("form.label")}, ${inCard("form.field")} > ${p("form.hint")} { grid-column: 1; }
${inCard("form.field")} > ${p("form.label")} { align-self: end; font-size: ${T.body}; font-weight: ${T.medium}; }
${inCard("form.field")} > ${p("form.hint")} { align-self: start; }
${inCard("form.field")}:not(:has(> ${p("form.hint")})) > ${p("form.label")} { align-self: center; }
${inCard("form.field")} > ${p("form.input")}, ${inCard("form.field")} > ${p("form.select")} {
  grid-column: 2; grid-row: 1 / span 2; justify-self: stretch; width: var(${CONTROL_WIDTH_VAR}, 100%); min-width: 0; max-width: 100%;
}
${inCard("form.field")} > [data-width] { justify-self: start; }
${inCard("form.field")}:not(:has(> ${p("form.hint")})) > ${p("form.input")},
${inCard("form.field")}:not(:has(> ${p("form.hint")})) > ${p("form.select")} { grid-row: 1; }
${inCard("form.field")} > ${p("form.textarea")} { width: 100%; }
${inCard("switch-field")}, ${inCard("form.check")} { padding-block: ${T.sm}; }
:where(${p("settings-page.card")} section)[data-part] { display: flex; flex-direction: column; gap: ${T.sm}; min-width: 0; }
:where(${p("settings-page.card")} section[data-part]) > ${p("form.button")} { align-self: flex-start; }
${inCard("form.section.title")} { padding-inline: 0; }
${inCard("form.field")} + ${p("form.field")}, ${inCard("form.field")} + ${p("switch-field")}, ${inCard("form.field")} + ${p("form.check")},
${inCard("switch-field")} + ${p("form.field")}, ${inCard("switch-field")} + ${p("switch-field")}, ${inCard("switch-field")} + ${p("form.check")},
${inCard("form.check")} + ${p("form.field")}, ${inCard("form.check")} + ${p("switch-field")}, ${inCard("form.check")} + ${p("form.check")} {
  border-top: 1px solid ${T.border};
}
${inCard("card")}, ${inCard("form.section")} { padding: ${T.md} 0 0; border: none; border-radius: 0; background-color: transparent; }
${inCard("card")} + ${p("card")}, ${inCard("form.section")} + ${p("form.section")} { margin-top: ${T.sm}; border-top: 1px solid ${T.border}; }
${p("settings-page.card")} > ${p("appearance")} { max-width: none; padding: 0; overflow: visible; }
[data-settings-found] {
  border-radius: ${T.rSm}; outline: 2px solid ${T.accent}; outline-offset: 2px;
  background-color: ${T.accentSoft}; transition: background-color ${T.fast} ${T.ease};
}
@container ${FIELDS_CONTAINER} (max-width: ${FIELDS_NARROW}) {
  ${inCard("form.field")} { grid-template-columns: minmax(0, 1fr); align-items: stretch; }
  ${inCard("form.field")} > *, ${inCard("form.field")} > ${p("form.label")}, ${inCard("form.field")} > ${p("form.hint")} { grid-column: 1; grid-row: auto; align-self: auto; }
  ${inCard("form.field")} > ${p("form.input")}, ${inCard("form.field")} > ${p("form.select")},
  ${inCard("form.field")}:not(:has(> ${p("form.hint")})) > ${p("form.input")},
  ${inCard("form.field")}:not(:has(> ${p("form.hint")})) > ${p("form.select")} { grid-column: 1; grid-row: 2; }
  ${inCard("form.field")} > ${p("form.hint")} { grid-row: 3; }
}`;

/**
 * P1 (WP191): the page measures its own width, not the viewport's — a
 * narrow window, a pop-out or the rail beside it make it narrow too.
 */
/** Below this content width the settings cards stack their columns. */
const ONE_COLUMN = "40rem";

const NARROW = `${p("settings-page.content")} { container: nail-settings-content / inline-size; }
@container nail-settings-content (max-width: ${ONE_COLUMN}) {
  ${p("settings-page.header")} { padding: ${T.lg} ${T.lg} ${T.sm}; }
  ${p("settings-page.body")} { padding: ${T.xs} ${T.lg} ${T.xl}; }
}
@container nail-settings-content (max-width: 30rem) {
  ${p("settings-page.header")} { padding: ${T.md} ${T.md} ${T.sm}; }
  ${p("settings-page.body")} { padding: ${T.xs} ${T.md} ${T.lg}; gap: ${T.md}; }
  ${p("settings-page.card")} { padding: ${T.md}; }
}`;

/**
 * M5a review: the Density control keeps its width (High contrast fills the
 * selection app-wide, in `theme/skin.ts`).
 */
const SEGMENTED = `${p("appearance")} ${p("segmented")}, ${p("settings-page")} ${p("segmented")} { justify-self: start; width: max-content; max-width: 100%; }
${p("appearance")} ${p("segmented.option")}[aria-checked="true"], ${p("settings-page")} ${p("segmented.option")}[aria-checked="true"] {
  box-shadow: inset 0 0 0 1px ${T.border}, var(--nail-shadow-raised);
}`;

/** The whole settings-page skin. */
export const SETTINGS_SKIN_CSS = [PAGE, NAV, CARDS, NARROW, SEGMENTED].join("\n");
