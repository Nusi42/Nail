/**
 * The look of the M5d Appearance additions and the theme editor (WP183),
 * on the token schema only (`var(--nail-…)`), like `../../theme/skin.ts`.
 * The theme compiler emits it after the shared skin and before the theme's
 * part rules (integration: `theme/compile.ts`), so `theme.kdl` keeps the
 * last word. Buttons, fields, tabs and tags are the shared primitives; this
 * adds the arrangement of the editor (header, tabs beside the panel, the
 * preview beside both, stacking below a narrow window), the token and part
 * rows, the slider rows and the preview's miniature window (whose colours
 * come from geometry rules: the edited theme's own values).
 */

import { FIELDS_NARROW } from "../settings/skin";
import { FIELDS_CONTAINER } from "../../theme/skinResponsive";
import { p } from "../../theme/selectors";

/** The editor's own parts. */
const e = (part: string) => p(`theme-editor.${part}`);
/** The preview's parts. */
const v = (part: string) => p(`theme-editor.preview.${part}`);

/** Token shorthands. */
const T = {
  bg: "var(--nail-color-bg)",
  subtle: "var(--nail-color-bg-subtle)",
  raised: "var(--nail-color-bg-raised)",
  fg: "var(--nail-color-fg)",
  muted: "var(--nail-color-fg-muted)",
  border: "var(--nail-color-border)",
  line: "var(--nail-color-border-strong, var(--nail-color-border))",
  accent: "var(--nail-color-accent)",
  warning: "var(--nail-color-warning)",
  accentSoft:
    "var(--nail-color-accent-soft, color-mix(in srgb, var(--nail-color-accent) 14%, var(--nail-color-bg)))",
  selection: "var(--nail-color-selection)",
  selectionFg: "var(--nail-color-selection-fg)",
  focus: "var(--nail-color-focus-ring)",
  mono: "var(--nail-font-mono)",
  label: "var(--nail-font-size-label)",
  small: "var(--nail-font-size-small)",
  semibold: "var(--nail-font-weight-semibold)",
  xxs: "var(--nail-space-xxs)",
  xs: "var(--nail-space-xs)",
  sm: "var(--nail-space-sm)",
  md: "var(--nail-space-md)",
  lg: "var(--nail-space-lg)",
  xl: "var(--nail-space-xl)",
  control: "var(--nail-size-control)",
  rSm: "var(--nail-radius-sm)",
  rMd: "var(--nail-radius-md)",
  rLg: "var(--nail-radius-lg)",
  pill: "var(--nail-radius-pill)",
  fast: "var(--nail-motion-fast)",
  ease: "var(--nail-motion-ease)",
} as const;

/** The container the editor's layout query measures. */
export const THEME_EDITOR_CONTAINER = "nail-theme-editor";
/**
 * Below this width the tabs go above the panel and the preview beside it at
 * a larger share (P1 review: at 1280 px the third column squeezed it).
 */
export const THEME_EDITOR_MEDIUM = "80rem";
/**
 * Below this width everything is one column with the preview *above* the
 * panel, so it stays in sight while tokens are edited (P1 review: below the
 * token list it was off screen).
 */
export const THEME_EDITOR_NARROW = "52rem";
/** The container the preview's narrow rule measures (P1 WP192). */
export const THEME_PREVIEW_CONTAINER = "nail-theme-preview";
/** Below this width a token's value field goes below its name (P1 WP192). */
export const THEME_EDITOR_STACKED = "40rem";
/** The container the token rows measure: the panel beside the preview (P1 review). */
export const THEME_PANEL_CONTAINER = "nail-theme-panel";
/** Below this panel width a token's value field goes below its name (P1 review). */
export const THEME_PANEL_STACKED = "40rem";

/** A field of the light / dark pair. */
const PAIR_FIELD = `${p("appearance.pair")} > ${p("form.field")}`;

const focusRing = `outline: 2px solid ${T.focus}; outline-offset: 2px;`;

export const APPEARANCE_SKIN_CSS = `${p("appearance.header")} { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: ${T.sm}; }
${p("appearance.history")} { display: flex; flex-wrap: wrap; align-items: center; gap: ${T.xs}; }
${p("appearance.history")} > ${p("form.error")} { display: flex; flex-wrap: wrap; align-items: center; gap: ${T.sm}; flex-basis: 100%; }
${p("appearance.actions")}[data-actions="themes"] { justify-content: flex-start; flex-wrap: wrap; }
/* P1 (WP192): the light / dark choice is one pair of fields — no settings-row line between them. */
${p("appearance.pair")} > ${p("form.field")} + ${p("form.field")} { border-top: 0; }
/* P1 verify: where a card puts labels beside their controls, each field of the pair gives its label
   its own width on one line and the select the rest — the label column never shrinks under its text
   ("Light t" / "Dark th" at 1024 px). The pair stacks its two fields when they do not fit side by side
   (\`theme/skin.ts\`); a narrow card puts every label above its control, the pair's too. */
@container ${FIELDS_CONTAINER} (width > ${FIELDS_NARROW}) {
  ${PAIR_FIELD} { grid-template-columns: max-content minmax(0, 1fr); }
  ${PAIR_FIELD} > ${p("form.label")} { grid-column: 1; white-space: nowrap; }
  ${PAIR_FIELD} > ${p("form.select")} { grid-column: 2; grid-row: 1; }
}
${p("appearance.slider")} { display: flex; align-items: center; gap: ${T.md}; }
${p("appearance.range")} { flex: 1 1 auto; min-width: 0; accent-color: ${T.accent}; }
${p("appearance.range")}:focus-visible { ${focusRing} }
${p("appearance.range-value")} { min-width: calc(8 * ${T.md}); font-size: ${T.small}; color: ${T.muted}; font-variant-numeric: tabular-nums; }
/* ADR 0161: why the accent is off while a high-contrast theme is active. */
${p("appearance.accent-note")} { margin: 0; font-size: ${T.small}; color: ${T.muted}; }

${p("theme-editor")} { container: ${THEME_EDITOR_CONTAINER} / inline-size; display: flex; flex-direction: column; gap: ${T.md}; min-height: 0; }
${e("host")} { display: flex; flex-direction: column; min-height: 0; padding: ${T.lg}; }
${e("header")} { display: flex; flex-wrap: wrap; align-items: flex-end; gap: ${T.sm} ${T.md}; }
${e("header")} > ${p("form.section.title")} { flex-basis: 100%; margin: 0; }
${e("actions")} { display: flex; flex-wrap: wrap; align-items: center; gap: ${T.sm}; }
${e("status")} { margin-inline-start: auto; font-size: ${T.small}; color: ${T.muted}; }
${e("notice")} { margin: 0; padding: ${T.sm} ${T.md}; border-radius: ${T.rMd}; background-color: ${T.accentSoft}; color: ${T.fg}; }
${e("notice")}[data-tone="warning"] { border: 1px solid ${T.warning}; background-color: color-mix(in srgb, ${T.warning} 14%, ${T.bg}); }
${e("body")} { display: grid; grid-template-columns: minmax(calc(8 * ${T.lg}), auto) minmax(0, 3fr) minmax(calc(18 * ${T.lg}), 2fr); grid-template-areas: "tabs panel preview"; gap: ${T.lg}; align-items: start; min-height: 0; }
${e("body")} > ${p("tabs.tab-list")} { grid-area: tabs; display: flex; flex-direction: column; gap: ${T.xxs}; border: 0; }
${e("body")} > ${e("panel")} { grid-area: panel; }
${e("body")} > ${e("preview")} { grid-area: preview; }
${e("body")} > ${p("tabs.tab-list")} > ${p("tabs.tab")} { justify-content: flex-start; text-align: start; }
${e("panel")} { container: ${THEME_PANEL_CONTAINER} / inline-size; display: flex; flex-direction: column; gap: ${T.md}; min-width: 0; }
${e("tokens")} { display: flex; flex-direction: column; gap: ${T.sm}; }
${e("token-list")}, ${e("part-list")} { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; border: 1px solid ${T.border}; border-radius: ${T.rMd}; }
/* P1 (WP192): one grid per row — name, origin tag, controls — with the controls at a fixed share of
   the row, so every row's tag ends at the same edge and the value field gets room for long values.
   Override notes go below the name. */
${e("token")} { display: grid; grid-template-columns: minmax(0, 1fr) auto minmax(0, 60%); gap: ${T.xs} ${T.md}; align-items: center; padding: ${T.sm} ${T.md}; }
${e("token")} + ${e("token")} { border-top: 1px solid ${T.border}; }
${e("token")}[data-state~="changed"] { background-color: ${T.accentSoft}; }
${e("token.text")} { display: contents; }
${e("token.text")} > ${e("token.name")} { grid-column: 1; grid-row: 1; min-width: 0; }
${e("token.text")} > ${p("tag")}[data-kind="origin"] { grid-column: 2; grid-row: 1; justify-self: end; white-space: nowrap; }
${e("token.text")} > :nth-child(n + 3) { grid-column: 1 / 3; justify-self: start; }
${e("token.name")}, ${e("part.selector")}, ${e("property.name")}, ${e("property.text")} { font-family: ${T.mono}; font-size: ${T.small}; overflow-wrap: anywhere; }
${e("token.controls")} { grid-column: 3; grid-row: 1; display: flex; align-items: center; gap: ${T.xs}; min-width: 0; }
${e("token.controls")} > ${p("form.input")} { flex: 1 1 auto; width: auto; min-width: 0; }
${e("token.color")} { width: calc(${T.control} + ${T.md}); height: ${T.control}; padding: ${T.xxs}; border: 1px solid ${T.line}; border-radius: ${T.rSm}; background: none; }
${e("token.color")}:focus-visible { ${focusRing} }
${e("parts")} { display: grid; grid-template-columns: minmax(calc(12 * ${T.lg}), 1fr) minmax(0, 2fr); gap: ${T.lg}; align-items: start; }
${e("part-picker")} { display: flex; flex-direction: column; gap: ${T.sm}; min-width: 0; }
${e("part-list")} { max-height: calc(24 * ${T.lg}); overflow: auto; }
${e("part-button")} { display: block; width: 100%; padding: ${T.xs} ${T.sm}; border: 0; background: none; color: ${T.fg}; font-family: ${T.mono}; font-size: ${T.small}; text-align: start; overflow-wrap: anywhere; cursor: pointer; border-radius: ${T.rSm}; transition: background-color ${T.fast} ${T.ease}; }
${e("part-button")}:hover { background-color: ${T.raised}; }
${e("part-button")}:focus-visible { ${focusRing} }
${e("part-button")}[aria-pressed="true"] { background-color: ${T.selection}; color: ${T.selectionFg}; }
${e("part-button")}[data-state~="own"] { font-weight: ${T.semibold}; }
${e("part")} { display: flex; flex-direction: column; gap: ${T.md}; min-width: 0; }
${e("part.header")} { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: ${T.sm}; }
${e("rule")} { display: flex; flex-direction: column; gap: ${T.xs}; padding: ${T.sm} ${T.md}; border: 1px solid ${T.border}; border-radius: ${T.rMd}; }
${e("properties")} { margin: 0; display: flex; flex-direction: column; gap: ${T.xs}; }
${e("property")} { display: grid; grid-template-columns: minmax(calc(8 * ${T.lg}), auto) minmax(0, 1fr); gap: ${T.md}; align-items: center; }
${e("property")}[data-state~="overridden"] ${e("property.text")} { text-decoration: line-through; color: ${T.muted}; }
${e("property.value")} { margin: 0; display: flex; align-items: center; gap: ${T.xs}; min-width: 0; }
${e("property.value")} > ${p("form.input")} { flex: 1 1 auto; min-width: 0; }
${e("add-property")}, ${e("add-part")} { display: flex; flex-wrap: wrap; align-items: flex-end; gap: ${T.sm}; }
${e("add-property")} > ${p("form.error")}, ${e("add-part")} > ${p("form.error")} { flex-basis: 100%; }
${e("empty")}, ${e("loading")}, ${e("unavailable")} { margin: 0; color: ${T.muted}; }

${e("preview")} { margin: 0; display: flex; flex-direction: column; gap: ${T.xs}; position: sticky; top: 0; container: ${THEME_PREVIEW_CONTAINER} / inline-size; }
${e("preview.caption")} { font-size: ${T.label}; color: ${T.muted}; }
${v("frame")} { display: grid; grid-template-columns: ${T.lg} minmax(0, 2fr) minmax(0, 3fr) minmax(0, 4fr); min-height: calc(16 * ${T.lg}); overflow: hidden; border: 1px solid; border-radius: ${T.rLg}; }
${v("rail")} { display: flex; flex-direction: column; align-items: center; padding-block: ${T.sm}; border-inline-end: 1px solid; }
${v("rail-item")} { width: ${T.sm}; height: ${T.sm}; }
${v("sidebar")} { display: flex; flex-direction: column; min-width: 0; }
${v("folder")} { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
${v("list")} { display: flex; flex-direction: column; min-width: 0; border-inline: 1px solid; }
${v("row")} { position: relative; display: grid; grid-template-columns: auto minmax(0, 1fr); align-items: center; border-bottom: 1px solid; }
${v("avatar")} { display: grid; place-items: center; width: calc(2 * ${T.md}); height: calc(2 * ${T.md}); border-radius: ${T.pill}; }
${v("row-text")} { display: flex; flex-direction: column; min-width: 0; }
${v("from")}, ${v("snippet")} { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
${v("unread")} { position: absolute; inset-inline-start: ${T.xxs}; top: 50%; width: ${T.xs}; height: ${T.xs}; border-radius: ${T.pill}; }
${v("reader")} { display: flex; flex-direction: column; align-items: flex-start; min-width: 0; }
${v("title")}, ${v("body")} { overflow-wrap: anywhere; }
${v("actions")} { display: flex; flex-wrap: wrap; gap: ${T.xs}; }
${v("secondary")}, ${v("field")} { border: 1px solid; }
${v("field")} { align-self: stretch; }

${p("theme-dialog")} { display: flex; flex-direction: column; gap: ${T.md}; }
${p("theme-dialog.actions")} { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: ${T.sm}; margin-top: auto; }
${p("theme-dialog.question")} { margin: 0; font-weight: ${T.semibold}; }
${p("theme-dialog.note")}, ${p("theme-dialog.gone")} { margin: 0; color: ${T.muted}; }

/* A narrow preview leaves out its folder column so the list and the message keep readable widths. */
@container ${THEME_PREVIEW_CONTAINER} (max-width: 22rem) {
  ${v("frame")} { grid-template-columns: ${T.lg} minmax(0, 1fr) minmax(0, 1fr); }
  ${v("sidebar")} { display: none; }
}
@container ${THEME_EDITOR_CONTAINER} (max-width: ${THEME_EDITOR_STACKED}) {
  ${e("token")} { grid-template-columns: minmax(0, 1fr) auto; }
  ${e("token.controls")} { grid-column: 1 / -1; grid-row: auto; }
}
/* The panel shares its row with the preview: its rows stack by the panel's own width. */
@container ${THEME_PANEL_CONTAINER} (max-width: ${THEME_PANEL_STACKED}) {
  ${e("token")} { grid-template-columns: minmax(0, 1fr) auto; }
  ${e("token.controls")} { grid-column: 1 / -1; grid-row: auto; }
}
@container ${THEME_EDITOR_CONTAINER} (max-width: ${THEME_EDITOR_MEDIUM}) {
  ${e("body")} { grid-template-columns: minmax(0, 3fr) minmax(calc(16 * ${T.lg}), 2fr); grid-template-areas: "tabs tabs" "panel preview"; }
  ${e("body")} > ${p("tabs.tab-list")} { flex-direction: row; flex-wrap: wrap; }
  ${e("parts")} { grid-template-columns: minmax(0, 1fr); }
}
@container ${THEME_EDITOR_CONTAINER} (max-width: ${THEME_EDITOR_NARROW}) {
  ${e("body")} { grid-template-columns: minmax(0, 1fr); grid-template-areas: "tabs" "preview" "panel"; }
  ${e("preview")} { position: static; }
  ${v("frame")} { min-height: calc(10 * ${T.lg}); }
}
`;
