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
 *   footer sticks to the page's bottom edge, over the page's bottom padding
 *   (`SETTINGS_BODY_END`), so Save / Test / Run stay in view.
 * - **Text view** (item 14): the highlighted copy and the textarea share
 *   one grid cell, the same font and padding, so the colours sit under the
 *   typed text; the textarea's own text is transparent.
 * - **Compact rows** (item 9): the labels of a condition or action row are
 *   visually hidden under a row of the same kind (same fields, same
 *   places; each field keeps its name); a row of another kind shows its
 *   own labels, so no label stands over another field.
 * - **Footer** (R6, ADR 0239): the shared `dialog.footer` sits inside the
 *   editor's own sticky footer, so it does not stick or draw its own edge
 *   there (the window body's last button row would); it spans the footer,
 *   so Save rule stays at the end whatever Run says.
 * - **"Saved"** fades after a moment (item 2); the problem list inside the
 *   shared problem indicator's details.
 * - **Exchange badge** (R3): one ⓘ per rule; its popover lists each
 *   account's line on hover and while the rule's row has the keyboard
 *   focus, until Escape hides it (`data-popover="hidden"` on the row). Otherwise the lines stay visually hidden (not removed), so the
 *   row's description still reads them; the popover lies over the list
 *   under its row, as wide as the row, so it moves nothing and the list's
 *   edge never clips it.
 * - **Rule list** (R3): the row's "Off" is for screen readers only (the
 *   row looks muted); the On mark shows its check only while the rule is
 *   on.
 */
import { p, ps } from "../../theme/selectors";
import { INLINE_ROW, VISUALLY_HIDDEN } from "../../theme/declarations";
import { T } from "../../theme/tokens";
import { SETTINGS_BODY_END } from "../settings/skin";
import { ACTION_KINDS, CONDITION_KINDS, GROUP_KINDS } from "./model";

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
${inRules("rules.footer")} > ${p("dialog.footer")}:last-child {
  position: static; flex: 1 1 100%; padding: 0; border: none; background: none;
}
${inRules("rules.footer.tools")} { display: flex; flex-wrap: wrap; align-items: center; gap: ${T.sm}; }
${p("rules.issues.items")} { margin: 0; padding-left: ${T.lg}; }
:is(${ps("rules.text.gutter", "rules.empty.list", "exchange.rule-badge.reasons")}) { list-style: none; }
${p("rules.issues.item")} { overflow-wrap: anywhere; }
${inRules("rules.footer.state")}[data-state="saved"] { animation: nail-rules-fade ${T.fast} ${SAVED_FADE_DELAY} both; }
@keyframes nail-rules-fade { to { opacity: 0; } }
@media (prefers-reduced-motion: reduce) {
  ${inRules("rules.footer.state")}[data-state="saved"] { animation: none; }
}
${inRules("rules.run.why")} { ${VISUALLY_HIDDEN} }
${inRules("form.button")}[aria-disabled="true"] { opacity: ${T.disabled}; }`;

/**
 * The labels of a row that the row above shows already: only under a row
 * of the same kind (the same fields in the same places); a row of another
 * kind shows its own labels.
 */
export function repeatedRowLabels(): string[] {
  const condition = (kind: string) => `${p("rules.condition")}[data-kind="${kind}"]`;
  const action = (kind: string) => `${p("rules.action")}[data-kind="${kind}"]`;
  const groups: readonly string[] = GROUP_KINDS;
  return [
    ...CONDITION_KINDS.filter((kind) => !groups.includes(kind)).map(
      (kind) =>
        `${p("rules.group.item")}:has(> ${condition(kind)}) + ${p("rules.group.item")} > ${condition(kind)} ${p("form.label")}`,
    ),
    ...ACTION_KINDS.map((kind) => `${action(kind)} + ${action(kind)} ${p("form.label")}`),
  ];
}

const ROWS = `:is(${ps("rules.condition", "rules.action")}) ${p("form.label")} { font-size: ${T.small}; color: ${T.muted}; }
${repeatedRowLabels().join(",\n")} { ${VISUALLY_HIDDEN} }
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

/** Where the badge's popover shows: the ⓘ under the pointer, or its row with the keyboard focus. */
export const BADGE_OPEN = `:is(${p("exchange.rule-badge")}:hover, ${p("rules.list.item")}:focus-visible:not([data-popover="hidden"]) ${p("exchange.rule-badge")})`;

/* The popover spans its row (the list's width), so the list never clips it. */
const BADGES = `${p("rules.list.item")} { position: relative; }
${p("exchange.rule-badge")} { ${INLINE_ROW} }
${p("exchange.rule-badge.reasons")} { ${VISUALLY_HIDDEN} }
${BADGE_OPEN} > ${p("exchange.rule-badge.reasons")} {
  inset-inline: 0; top: 100%; z-index: 3; width: auto; height: auto; margin: 0;
  overflow: visible; clip-path: none; white-space: normal;
}
${p("rules.list.off")} { ${VISUALLY_HIDDEN} }
${p("rules.list.enabled")}[data-enabled="false"] > * { visibility: hidden; }`;

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
/* The page scrolls, not the widget: everything flows, and the footer sticks to the page's
   bottom edge. It reaches over the page's bottom padding and takes that padding inside, so
   no form shows under it and the buttons sit where the page's content ends. */
${p("settings-page.card")} ${p("rules")} { height: auto; }
${inCardRules("rules.footer")} {
  bottom: calc(-1 * var(${SETTINGS_BODY_END}, 0px)); padding-bottom: var(${SETTINGS_BODY_END}, 0px);
}
:is(${inCardRules("rules.editor.body")}, ${inCardRules("rules.side")}, ${inCardRules("rules.lower.panel")}) { overflow: visible; }
${inCardRules("rules.lower")} { max-height: none; }`;

const STACKED = `@container ${RULES_CONTAINER} (max-width: ${RULES_STACKED}) {
  ${inRules("rules.panes")} { grid-template-columns: minmax(0, 1fr); grid-template-rows: fit-content(35%) minmax(0, 1fr); }
  ${inRules("rules.form")} { grid-template-columns: minmax(0, 1fr); }
  ${inRules("rules.form.label")} { padding-top: 0; }
}`;

/** The rules skin. */
export const RULES_SKIN_CSS = [STRUCTURE, ROWS, TEXT, BADGES, SETTINGS, STACKED].join("\n");
