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

/** `[data-part="…"]` for one part. */
const p = (part: string) => `[data-part="${part}"]`;
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
/** Below this width the preview goes below the panel. */
export const THEME_EDITOR_NARROW = "52rem";

const focusRing = `outline: 2px solid ${T.focus}; outline-offset: 2px;`;

export const APPEARANCE_SKIN_CSS = `${p("appearance.header")} { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: ${T.sm}; }
${p("appearance.history")} { display: flex; flex-wrap: wrap; align-items: center; gap: ${T.xs}; }
${p("appearance.history")} > ${p("form.error")} { display: flex; flex-wrap: wrap; align-items: center; gap: ${T.sm}; flex-basis: 100%; }
${p("appearance.actions")}[data-actions="themes"] { justify-content: flex-start; flex-wrap: wrap; }
${p("appearance.slider")} { display: flex; align-items: center; gap: ${T.md}; }
${p("appearance.range")} { flex: 1 1 auto; min-width: 0; accent-color: ${T.accent}; }
${p("appearance.range")}:focus-visible { ${focusRing} }
${p("appearance.range-value")} { min-width: calc(8 * ${T.md}); font-size: ${T.small}; color: ${T.muted}; font-variant-numeric: tabular-nums; }

${p("theme-editor")} { container: ${THEME_EDITOR_CONTAINER} / inline-size; display: flex; flex-direction: column; gap: ${T.md}; min-height: 0; }
${e("host")} { display: flex; flex-direction: column; min-height: 0; padding: ${T.lg}; }
${e("header")} { display: flex; flex-wrap: wrap; align-items: flex-end; gap: ${T.sm} ${T.md}; }
${e("header")} > ${p("form.section.title")} { flex-basis: 100%; margin: 0; }
${e("actions")} { display: flex; flex-wrap: wrap; align-items: center; gap: ${T.sm}; }
${e("status")} { margin-inline-start: auto; font-size: ${T.small}; color: ${T.muted}; }
${e("notice")} { margin: 0; padding: ${T.sm} ${T.md}; border-radius: ${T.rMd}; background-color: ${T.accentSoft}; color: ${T.fg}; }
${e("notice")}[data-tone="warning"] { border: 1px solid ${T.warning}; background-color: color-mix(in srgb, ${T.warning} 14%, ${T.bg}); }
${e("body")} { display: grid; grid-template-columns: minmax(calc(10 * ${T.lg}), auto) minmax(0, 1fr) minmax(calc(16 * ${T.lg}), 1fr); gap: ${T.lg}; align-items: start; min-height: 0; }
${e("body")} > ${p("tabs.tab-list")} { display: flex; flex-direction: column; gap: ${T.xxs}; border: 0; }
${e("body")} > ${p("tabs.tab-list")} > ${p("tabs.tab")} { justify-content: flex-start; text-align: start; }
${e("panel")} { display: flex; flex-direction: column; gap: ${T.md}; min-width: 0; }
${e("tokens")} { display: flex; flex-direction: column; gap: ${T.sm}; }
${e("token-list")}, ${e("part-list")} { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; border: 1px solid ${T.border}; border-radius: ${T.rMd}; }
${e("token")} { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: ${T.sm} ${T.md}; align-items: center; padding: ${T.sm} ${T.md}; }
${e("token")} + ${e("token")} { border-top: 1px solid ${T.border}; }
${e("token")}[data-state~="changed"] { background-color: ${T.accentSoft}; }
${e("token.text")} { display: flex; flex-wrap: wrap; align-items: center; gap: ${T.xs} ${T.sm}; min-width: 0; }
${e("token.name")}, ${e("part.selector")}, ${e("property.name")}, ${e("property.text")} { font-family: ${T.mono}; font-size: ${T.small}; overflow-wrap: anywhere; }
${e("token.controls")} { display: flex; align-items: center; gap: ${T.xs}; }
${e("token.controls")} > ${p("form.input")} { width: calc(12 * ${T.lg}); max-width: 100%; }
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

${e("preview")} { margin: 0; display: flex; flex-direction: column; gap: ${T.xs}; position: sticky; top: 0; }
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

@container ${THEME_EDITOR_CONTAINER} (max-width: ${THEME_EDITOR_NARROW}) {
  ${e("body")} { grid-template-columns: minmax(0, 1fr); }
  ${e("body")} > ${p("tabs.tab-list")} { flex-direction: row; flex-wrap: wrap; }
  ${e("parts")} { grid-template-columns: minmax(0, 1fr); }
  ${e("preview")} { position: static; }
}
`;
