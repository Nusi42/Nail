/**
 * The rules window's structure on the token schema only (`var(--nail-…)`),
 * like `../../theme/skin.ts`; the theme compiler emits it after the shared
 * skin, so `theme.kdl`'s part rules (the look) still win where they style
 * the same property. What lives here is what a theme should not have to
 * repeat (R2 design review, ADR 0222):
 *
 * - **Responsive** (item 10): the rules widget is a size container; below
 *   `RULES_STACKED` the list goes over the editor and keeps a short,
 *   scrolling height, so the editor and its footer keep the room — in the
 *   window and in the Settings card alike (item 15).
 * - **Footer always in view** (item 4): in the window the editor's body and
 *   lower panel scroll and the footer stays (it also sticks to the bottom
 *   of a scrolling editor); in the Settings card the page scrolls and the
 *   footer ends the editor.
 * - **Text view** (item 14): the highlighted copy and the textarea share
 *   one grid cell, the same font and padding, so the colours sit under the
 *   typed text; the textarea's own text is transparent.
 * - **Compact rows** (item 9): the labels of condition and action rows are
 *   visually hidden after the first row (each field keeps its name).
 * - **Questions** (items 4, 5): over the whole window, centred.
 * - **"Saved"** fades after a moment (item 2); the problem list inside the
 *   shared problem indicator's details.
 * - **Exchange badges** (item 3): the reasons show beside the ⓘ on hover
 *   and focus.
 */
import { p, ps } from "../../theme/selectors";
import { VISUALLY_HIDDEN } from "../../theme/declarations";
import { T } from "../../theme/tokens";

/** The container the responsive rules measure. */
export const RULES_CONTAINER = "nail-rules";
/** Below this width the list goes over the editor. */
export const RULES_STACKED = "760px";
/** How long "Saved" stays before it fades. */
export const SAVED_FADE_DELAY = "4s";

const inRules = (part: string) => `${p("rules")} ${p(part)}`;

const STRUCTURE = `${p("rules")} { container: ${RULES_CONTAINER} / inline-size; position: relative; }
${inRules("rules.footer")} { position: sticky; bottom: 0; z-index: 1; flex-shrink: 0; }
${inRules("rules.header")} { flex-shrink: 0; }
${inRules("rules.lower")} { flex-basis: auto; }
${inRules("rules.confirm")} {
  position: absolute; inset: 0; z-index: 2; margin: auto; height: fit-content; width: min(28rem, calc(100% - 2 * ${T.md}));
}
${p("rules.issues.items")} { margin: 0; padding-left: ${T.lg}; }
:is(${ps("rules.text.gutter", "rules.empty.list", "exchange.rule-badges")}) { list-style: none; }
${p("rules.issues.item")} { overflow-wrap: anywhere; }
${inRules("rules.footer.state")}[data-state="saved"] { animation: nail-rules-fade ${T.fast} ${SAVED_FADE_DELAY} both; }
@keyframes nail-rules-fade { to { opacity: 0; } }
@media (prefers-reduced-motion: reduce) {
  ${inRules("rules.footer.state")}[data-state="saved"] { animation: none; }
}
${inRules("rules.run.why")} { ${VISUALLY_HIDDEN} }
/* "+ Add condition": the menu button's name (localized, its aria-label) beside its icon. */
${inRules("rules.add.button")}::after { content: attr(aria-label); margin-inline-start: ${T.xxs}; }
${inRules("form.button")}[aria-disabled="true"] { opacity: ${T.disabled}; }`;

const ROWS = `:is(${ps("rules.condition", "rules.action")}) ${p("form.label")} { font-size: ${T.small}; color: ${T.muted}; }
${p("rules.group.item")} + ${p("rules.group.item")} > ${p("rules.condition")} ${p("form.label")},
${p("rules.action")} + ${p("rules.action")} ${p("form.label")} { ${VISUALLY_HIDDEN} }
${p("rules.action")}::marker { color: ${T.muted}; }`;

const TEXT = `${inRules("rules.text.area")} { display: grid; min-width: 0; min-height: 0; position: relative; overflow: hidden; }
${inRules("rules.text.area")} > * {
  grid-area: 1 / 1; margin: 0; min-height: 100%; box-sizing: border-box;
  font-family: ${T.fontMono}; font-size: ${T.body}; line-height: ${T.normal}; tab-size: 4;
}
/* The copy keeps the textarea's border width (transparent), so the text lines up. */
${inRules("rules.text.highlight")} { overflow: hidden; pointer-events: none; border: ${T.controlBorder}; border-color: transparent; }
${inRules("rules.text.source")} { resize: none; overflow: auto; color: transparent; }
${inRules("rules.text.source")}::selection { color: transparent; background-color: ${T.accentSoft}; }
${inRules("rules.text.gutter")} { font-size: ${T.body}; line-height: ${T.normal}; }`;

const BADGES = `${p("exchange.rule-badge.reasons")} {
  position: absolute; inset-inline-end: 0; top: 100%; z-index: 3; width: max-content; max-width: 18rem; display: none;
}
${p("exchange.rule-badge")}:is(:hover, :focus-within) > ${p("exchange.rule-badge.reasons")} { display: block; }`;

/**
 * One look in Settings (item 15): the settings card lays its fields out as
 * label-left rows with separators; inside the rules widget they keep the
 * window's compact rows (these selectors are one part longer, so they win).
 */
const inCardRules = (part: string) => `${p("settings-page.card")} ${p("rules")} ${p(part)}`;
const SETTINGS = `${inCardRules("form.field")} {
  display: grid; grid-template-columns: minmax(0, 1fr); align-items: start; row-gap: ${T.xxs}; padding-block: 0; border-top: none;
}
${inCardRules("form.field")} > * { grid-column: 1; grid-row: auto; align-self: auto; }
${inCardRules("form.field")} > ${p("form.label")} { font-size: ${T.small}; font-weight: ${T.regular}; }
${inCardRules("form.field")} > ${p("form.label")} { grid-row: 1; }
${inCardRules("form.field")} > :is(${ps("form.input", "form.select")}) { grid-row: 2; width: 100%; }
:is(${inCardRules("form.check")}, ${inCardRules("switch-field")}) { padding-block: 0; border-top: none; }
/* The page scrolls, not the widget: everything flows, the footer ends the editor (a footer
   sticking to the page would leave the page's bottom padding showing the form under it). */
${p("settings-page.card")} ${p("rules")} { height: auto; }
${inCardRules("rules.footer")} { position: static; }
:is(${inCardRules("rules.editor.body")}, ${inCardRules("rules.side")}, ${inCardRules("rules.lower.panel")}) { overflow: visible; }
${inCardRules("rules.lower")} { max-height: none; }`;

const STACKED = `@container ${RULES_CONTAINER} (max-width: ${RULES_STACKED}) {
  ${inRules("rules.panes")} { grid-template-columns: minmax(0, 1fr); grid-template-rows: fit-content(35%) minmax(0, 1fr); }
  ${inRules("rules.form")} { grid-template-columns: minmax(0, 1fr); }
  ${inRules("rules.form.label")} { padding-top: 0; }
}`;

/** The rules skin. */
export const RULES_SKIN_CSS = [STRUCTURE, ROWS, TEXT, BADGES, SETTINGS, STACKED].join("\n");
