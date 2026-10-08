/**
 * The look of customize-layout mode (M5d WP181, the proposal's "Layout
 * editor"), on the token schema only (`var(--nail-…)`), like
 * `../../theme/skin.ts`: the compiler emits it after the shared skin and
 * before the theme's part rules, so any `theme.kdl` overrides it. Buttons
 * are the shared `form.button`; the banner sits on the accent colour.
 *
 * Parts:
 * - `layout-editor.frame` (`data-window`, `data-dragging` during a pointer drag):
 *   banner, body, footer
 * - `layout-editor.banner` (+ `.banner-icon`, `.title`, `.hint`,
 *   `.banner-actions`)
 * - `layout-editor.body`: `layout-editor.canvas` (the window) beside
 *   `layout-editor.palette`
 * - `layout-editor.pane` (`data-state` = `hidden` / `picked` / `target`):
 *   `.pane-header` with `layout-editor.handle` (+ `.icon`; `aria-pressed`
 *   while picked up), `.pane-title`, `.pane-badge` (Hidden), `.pane-lock`;
 *   `.pane-body` (the real pane, inert)
 * - `layout-editor.targets` over a pane while something is picked up:
 *   `layout-editor.target` (`data-zone` = `left` / `right` / `top` /
 *   `bottom` / `swap` / `into`, `data-state="hover"` under the pointer;
 *   `.icon`, `.label`)
 * - the palette: `.palette-title`, `.presets`, `.preset` (`aria-pressed` =
 *   the arrangement shown now), `.preset-preview` (`.group` with
 *   `data-orientation`, `.pane`), `.preset-name`, `.widgets` (`.item`),
 *   `.widget` (`.widget-icon`, `.widget-text`, `.widget-name`,
 *   `.widget-description`)
 * - `layout-editor.footer` with `layout-editor.status` (`data-state="error"`)
 * - the settings card: `layout-editor.settings-actions` (the card itself
 *   is the shared `card`)
 */
import { p } from "../../theme/selectors";
import { QUIET_HOVER, smallTag } from "../../theme/declarations";

/** The panes of a preset's miniature: barely rounded at that size. */
const PREVIEW_RADIUS = "3px";

const T = {
  bg: "var(--nail-color-bg)",
  subtle: "var(--nail-color-bg-subtle)",
  raised: "var(--nail-color-bg-raised)",
  fg: "var(--nail-color-fg)",
  muted: "var(--nail-color-fg-muted)",
  border: "var(--nail-color-border)",
  accent: "var(--nail-color-accent)",
  accentFg: "var(--nail-color-accent-fg)",
  accentSoft:
    "var(--nail-color-accent-soft, color-mix(in srgb, var(--nail-color-accent) 14%, var(--nail-color-bg)))",
  danger: "var(--nail-color-danger)",
  focus: "var(--nail-color-focus-ring)",
  label: "var(--nail-font-size-label)",
  small: "var(--nail-font-size-small)",
  body: "var(--nail-font-size-body)",
  heading: "var(--nail-font-size-heading)",
  medium: "var(--nail-font-weight-medium)",
  semibold: "var(--nail-font-weight-semibold)",
  tight: "var(--nail-line-height-tight)",
  xxs: "var(--nail-space-xxs)",
  xs: "var(--nail-space-xs)",
  sm: "var(--nail-space-sm)",
  md: "var(--nail-space-md)",
  pane: "var(--nail-space-pane)",
  icon: "var(--nail-size-icon)",
  control: "var(--nail-size-control)",
  rSm: "var(--nail-radius-sm)",
  rMd: "var(--nail-radius-md)",
  pill: "var(--nail-radius-pill)",
  fast: "var(--nail-motion-fast)",
  ease: "var(--nail-motion-ease)",
  disabled: "var(--nail-opacity-disabled)",
} as const;

const focusRing = `outline: 2px solid ${T.focus}; outline-offset: 2px;`;

const FRAME = `${p("layout-editor.frame")} {
  display: grid; grid-template-rows: auto minmax(0, 1fr) auto; grid-template-columns: minmax(0, 1fr);
  min-width: 0; min-height: 0; height: 100%; color: ${T.fg}; background-color: ${T.bg};
}
${p("layout-editor.frame")}[data-dragging], ${p("layout-editor.frame")}[data-dragging] * { user-select: none; cursor: grabbing; }
${p("layout-editor.banner")} {
  display: flex; align-items: center; flex-wrap: wrap; gap: ${T.sm};
  padding: ${T.sm} ${T.pane}; color: ${T.accentFg}; background-color: ${T.accent}; font-size: ${T.small};
}
${p("layout-editor.banner-icon")} { flex: 0 0 auto; width: ${T.icon}; height: ${T.icon}; }
${p("layout-editor.title")} { margin: 0; font-size: ${T.body}; font-weight: ${T.semibold}; line-height: ${T.tight}; }
${p("layout-editor.title")}:focus-visible { outline: 2px solid ${T.accentFg}; outline-offset: 2px; }
/* P1: the hint takes a wide line; when the buttons do not fit beside it they wrap to their own row. */
${p("layout-editor.hint")} { flex: 1 1 28em; min-width: 0; }
${p("layout-editor.banner-actions")} { display: inline-flex; flex-wrap: wrap; gap: ${T.xs}; margin-inline-start: auto; }
${p("layout-editor.banner")} ${p("form.button")} {
  min-height: calc(${T.control} - ${T.xs}); padding: 0 ${T.sm};
  color: ${T.accentFg}; background-color: transparent;
  border: 1px solid color-mix(in srgb, ${T.accentFg} 45%, transparent);
}
${p("layout-editor.banner")} ${p("form.button")}:hover { background-color: color-mix(in srgb, ${T.accentFg} 16%, transparent); }
${p("layout-editor.banner")} ${p("form.button")}:focus-visible { outline: 2px solid ${T.accentFg}; outline-offset: 2px; }
${p("layout-editor.banner")} ${p("form.button")}[data-variant~="primary"] { color: ${T.accent}; background-color: ${T.accentFg}; border-color: transparent; }
${p("layout-editor.body")} {
  display: grid; grid-template-columns: minmax(0, 1fr) minmax(13rem, 16rem); min-width: 0; min-height: 0;
}
${p("layout-editor.canvas")} {
  display: grid; grid-template: minmax(0, 1fr) / minmax(0, 1fr); min-width: 0; min-height: 0;
  padding: ${T.sm}; overflow: auto; background-color: ${T.subtle};
}
/* P1: the window keeps the width its panes need beside the palette; the canvas scrolls sideways
   to the last pane instead of cutting it off. */
${p("layout-editor.canvas")} > * { min-width: min-content; min-height: 0; }
${p("layout-editor.footer")} {
  display: flex; align-items: center; flex-wrap: wrap; gap: ${T.sm};
  min-height: ${T.control}; padding: ${T.xxs} ${T.pane}; border-top: 1px solid ${T.border}; background-color: ${T.bg};
}
${p("layout-editor.status")} { flex: 1 1 auto; margin: 0; font-size: ${T.small}; color: ${T.muted}; }
${p("layout-editor.status")}[data-state~="error"] { color: ${T.danger}; }`;

const PANES = `${p("layout-editor.pane")} {
  position: relative; display: flex; flex-direction: column; box-sizing: border-box;
  min-width: 0; min-height: 0; height: 100%; overflow: hidden;
  border: 2px dashed color-mix(in srgb, ${T.accent} 55%, ${T.border}); border-radius: ${T.rMd};
  background-color: ${T.bg};
}
${p("layout-editor.pane")}[data-state~="picked"] { border-style: solid; border-color: ${T.accent}; background-color: ${T.accentSoft}; }
${p("layout-editor.pane")}[data-state~="hidden"] { height: auto; border-style: dotted; border-color: ${T.border}; }
${p("layout-editor.pane")}[data-state~="hidden"] > ${p("layout-editor.pane-body")} { display: none; }
${p("layout-editor.pane")}[data-state~="hidden"] ${p("layout-editor.pane-title")} { color: ${T.muted}; }
${p("layout-editor.pane-header")} {
  display: flex; align-items: center; flex: 0 0 auto; gap: ${T.xs};
  padding: ${T.xxs} ${T.xs}; border-bottom: 1px solid ${T.border}; background-color: ${T.subtle};
}
${p("layout-editor.handle")} {
  display: inline-flex; align-items: center; justify-content: center; flex: 0 0 auto;
  width: ${T.control}; height: ${T.control}; padding: 0; border: none; border-radius: ${T.rSm};
  color: ${T.muted}; background-color: transparent; cursor: grab; touch-action: none;
}
${p("layout-editor.handle")}:hover { ${QUIET_HOVER} }
${p("layout-editor.handle")}:focus-visible { ${focusRing} outline-offset: -2px; }
${p("layout-editor.handle")}[aria-pressed="true"] { color: ${T.accent}; background-color: ${T.accentSoft}; }
${p("layout-editor.handle")}:disabled { opacity: ${T.disabled}; }
${p("layout-editor.handle.icon")}, ${p("layout-editor.pane-lock")} { flex: 0 0 auto; width: ${T.icon}; height: ${T.icon}; }
${p("layout-editor.pane-lock")} { margin: 0 ${T.xs}; color: ${T.muted}; }
${p("layout-editor.pane-title")} {
  flex: 1 1 auto; min-width: 0; margin: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  font-size: ${T.small}; font-weight: ${T.semibold}; line-height: ${T.tight};
}
${p("layout-editor.pane-badge")} { ${smallTag(T.pill, T.fg, T.raised)} }
${p("layout-editor.pane-body")} { display: flex; flex-direction: column; flex: 1 1 auto; min-width: 0; min-height: 0; overflow: hidden; }
${p("layout-editor.pane-body")} > * { flex: 1 1 auto; min-height: 0; }
${p("layout-editor.targets")} {
  position: absolute; inset: 0; z-index: 3; display: grid; gap: ${T.xs}; padding: ${T.sm};
  grid-template-areas: ". top ." "left swap right" ". bottom ."; grid-template-columns: repeat(3, minmax(0, 1fr)); grid-template-rows: repeat(3, minmax(0, 1fr));
  background-color: color-mix(in srgb, ${T.bg} 72%, transparent);
}
${p("layout-editor.target")} {
  display: flex; flex-direction: column; align-items: center; justify-content: center; gap: ${T.xxs};
  min-width: 0; min-height: 0; padding: ${T.xxs}; overflow: hidden;
  border: 2px dashed ${T.accent}; border-radius: ${T.rMd};
  font-size: ${T.small}; font-weight: ${T.semibold}; color: ${T.accent}; background-color: ${T.bg};
  transition: background-color ${T.fast} ${T.ease};
}
${p("layout-editor.target")}[data-zone="top"] { grid-area: top; }
${p("layout-editor.target")}[data-zone="bottom"] { grid-area: bottom; }
${p("layout-editor.target")}[data-zone="left"] { grid-area: left; }
${p("layout-editor.target")}[data-zone="right"] { grid-area: right; }
${p("layout-editor.target")}[data-zone="swap"], ${p("layout-editor.target")}[data-zone="into"] { grid-area: swap; }
${p("layout-editor.target")}:hover, ${p("layout-editor.target")}[data-state~="hover"], ${p("layout-editor.target")}:focus-visible {
  border-style: solid; color: ${T.accentFg}; background-color: ${T.accent};
}
${p("layout-editor.target")}:focus-visible { ${focusRing} }
${p("layout-editor.target.icon")} { flex: 0 0 auto; width: ${T.icon}; height: ${T.icon}; }
${p("layout-editor.target.label")} { max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }`;

const PALETTE = `${p("layout-editor.palette")} {
  display: grid; align-content: start; gap: ${T.sm}; min-height: 0; overflow: auto;
  padding: ${T.md}; border-inline-start: 1px solid ${T.border}; background-color: ${T.subtle};
}
${p("layout-editor.palette-title")} { margin: ${T.xs} 0 0; font-size: ${T.body}; font-weight: ${T.semibold}; color: ${T.fg}; }
${p("layout-editor.presets")} { display: grid; grid-template-columns: repeat(auto-fill, minmax(6.5rem, 1fr)); gap: ${T.sm}; }
${p("layout-editor.presets")} > ${p("form.hint")} { grid-column: 1 / -1; margin: 0; }
${p("layout-editor.preset")} {
  display: grid; gap: ${T.xs}; padding: ${T.sm}; border: 1px solid ${T.border}; border-radius: ${T.rMd};
  font-size: ${T.small}; color: ${T.fg}; background-color: ${T.bg}; text-align: center;
}
${p("layout-editor.preset")}:hover { background-color: ${T.raised}; }
${p("layout-editor.preset")}:focus-visible { ${focusRing} }
${p("layout-editor.preset")}[aria-pressed="true"] { border-color: ${T.accent}; box-shadow: 0 0 0 1px ${T.accent}; font-weight: ${T.semibold}; }
${p("layout-editor.preset")}:disabled { opacity: ${T.disabled}; }
${p("layout-editor.preset-preview")} { display: flex; height: calc(${T.control} + ${T.xs}); }
${p("layout-editor.preset-preview.group")} { display: flex; flex: 1 1 0; gap: 2px; min-width: 0; min-height: 0; }
${p("layout-editor.preset-preview.group")}[data-orientation="vertical"] { flex-direction: column; }
${p("layout-editor.preset-preview.pane")} { flex: 1 1 0; min-width: 0; min-height: 0; border-radius: ${PREVIEW_RADIUS}; background-color: color-mix(in srgb, ${T.fg} 22%, ${T.bg}); }
${p("layout-editor.preset")}[aria-pressed="true"] ${p("layout-editor.preset-preview.pane")} { background-color: color-mix(in srgb, ${T.accent} 55%, ${T.bg}); }
${p("layout-editor.preset-name")} { min-width: 0; overflow-wrap: anywhere; }
${p("layout-editor.widgets.item")} { display: block; min-width: 0; }
${p("layout-editor.widgets")} { display: grid; gap: ${T.xs}; margin: 0; padding: 0; list-style: none; }
${p("layout-editor.widget")} {
  display: grid; grid-template-columns: auto minmax(0, 1fr); align-items: center; gap: ${T.sm}; width: 100%;
  padding: ${T.sm}; border: 1px solid ${T.border}; border-radius: ${T.rMd};
  font-size: ${T.small}; color: ${T.fg}; background-color: ${T.bg}; text-align: start; cursor: grab; touch-action: none;
}
${p("layout-editor.widget")}:hover { background-color: ${T.raised}; }
${p("layout-editor.widget")}:focus-visible { ${focusRing} }
${p("layout-editor.widget")}[aria-pressed="true"] { border-color: ${T.accent}; box-shadow: 0 0 0 1px ${T.accent}; }
${p("layout-editor.widget")}:disabled { opacity: ${T.disabled}; }
${p("layout-editor.widget-icon")} {
  display: grid; place-items: center; width: ${T.control}; height: ${T.control}; border-radius: ${T.rSm};
  color: ${T.accent}; background-color: ${T.accentSoft};
}
${p("layout-editor.widget-icon.svg")} { width: ${T.icon}; height: ${T.icon}; }
${p("layout-editor.widget-text")} { display: grid; gap: ${T.xxs}; min-width: 0; }
${p("layout-editor.widget-name")} { font-weight: ${T.semibold}; }
${p("layout-editor.widget-description")} { font-size: ${T.label}; color: ${T.muted}; }
${p("layout-editor.settings-actions")} { display: flex; flex-wrap: wrap; gap: ${T.sm}; }
${p("card")} > ${p("layout-editor.presets")} { grid-template-columns: repeat(auto-fill, minmax(7rem, 9rem)); }
${p("card")} > ${p("layout-editor.status")}:empty { display: none; }`;

/** The layout editor's skin (`../../theme/compile.ts` adds it after the shared skin). */
export const LAYOUT_EDITOR_SKIN_CSS = [FRAME, PANES, PALETTE].join("\n");
