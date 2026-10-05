/**
 * The look and structure of the toolbar customizer (M5d WP182), on the
 * token schema only (`var(--nail-…)`), like `../../theme/skin.ts`. Meant
 * for the theme compiler's skin list (after the shared skin, before the
 * theme's part rules), so any `theme.kdl` still overrides it. The window's
 * size comes from its window spec, never from here.
 *
 * Parts:
 * - `toolbar-customizer` (`data-toolbar`, `data-dragging` while something
 *   is dragged, `aria-busy` while a change is written), `.header`, `.intro`,
 *   `.body` (the lists left, the available actions right; one column in a
 *   narrow window), `.areas`, `.area` (`data-area` = `bar` / `more`),
 *   `.heading`, `.list` (`data-list` = `bar` / `more` / `palette`,
 *   `data-orientation`, `data-drop="end"` = dropping at its end), `.item`
 *   (`data-kind` = the item's type, `data-state="selected"`,
 *   `data-drop="before"`, `data-source="plugin"`) with `.item.icon`,
 *   `.item.label`, `.item.badge` (`data-badge` = `plugin` / `used`),
 *   `.empty`, `.commands`, `.palette` (`data-drop="remove"` while a bar
 *   item is dragged over it), `.drop-hint`, `.more-matches`, `.status`
 *   (`data-tone` = `done` / `error`), `.footer`, `.footer.spacer`
 * - `toolbar-customizer-settings` (the settings card): `.title`, `.intro`,
 *   `.empty`, `.list`, `.item`, `.name`, `.count`
 * - `toolbar[data-customizing]`: the live toolbar whose customizer is open
 */

import { CUSTOMIZER_PART, SETTINGS_PART } from "./ids";

/** `[data-part="…"]` for one part. */
const p = (part: string) => `[data-part="${part}"]`;
const c = (part: string) => p(`toolbar-customizer.${part}`);

/** Token shorthands (the same derived fallbacks as the shared skin). */
const T = {
  bg: "var(--nail-color-bg)",
  subtle: "var(--nail-color-bg-subtle)",
  raised: "var(--nail-color-bg-raised)",
  fg: "var(--nail-color-fg)",
  muted: "var(--nail-color-fg-muted)",
  border: "var(--nail-color-border)",
  accent: "var(--nail-color-accent)",
  accentSoft:
    "var(--nail-color-accent-soft, color-mix(in srgb, var(--nail-color-accent) 14%, var(--nail-color-bg)))",
  selection: "var(--nail-color-selection)",
  selectionFg: "var(--nail-color-selection-fg)",
  danger: "var(--nail-color-danger)",
  success: "var(--nail-color-success)",
  focus: "var(--nail-color-focus-ring)",
  small: "var(--nail-font-size-small)",
  label: "var(--nail-font-size-label)",
  semibold: "var(--nail-font-weight-semibold)",
  xxs: "var(--nail-space-xxs)",
  xs: "var(--nail-space-xs)",
  sm: "var(--nail-space-sm)",
  md: "var(--nail-space-md)",
  lg: "var(--nail-space-lg)",
  icon: "var(--nail-size-icon)",
  control: "var(--nail-size-control)",
  rSm: "var(--nail-radius-sm)",
  rMd: "var(--nail-radius-md)",
  pill: "var(--nail-radius-pill)",
  fast: "var(--nail-motion-fast)",
  ease: "var(--nail-motion-ease)",
} as const;

const focusRing = `outline: 2px solid ${T.focus}; outline-offset: 1px;`;

/** The container the customizer's narrow rule measures (P1 WP192). */
export const CUSTOMIZER_CONTAINER = "nail-toolbar-customizer";
/** Below this width the available actions go below the toolbar's lists. */
export const CUSTOMIZER_NARROW = "40rem";
/** Below this height the body scrolls instead of the lists shrinking further. */
export const CUSTOMIZER_SHORT = "30rem";

const FRAME = `${p(CUSTOMIZER_PART)} {
  display: flex; flex-direction: column; gap: ${T.md}; height: 100%; min-height: 0; padding: ${T.md};
  box-sizing: border-box; color: ${T.fg}; container: ${CUSTOMIZER_CONTAINER} / size;
}
${p(CUSTOMIZER_PART)}[aria-busy="true"] ${c("list")} { cursor: progress; }
${c("header")} { display: flex; flex-wrap: wrap; align-items: end; gap: ${T.sm} ${T.lg}; }
${c("intro")}, ${c("drop-hint")}, ${c("more-matches")} { margin: 0; color: ${T.muted}; font-size: ${T.small}; max-width: 70ch; }
/* P1 (WP192): in a tall window the lists share the height and scroll inside; in a short one (a
   container query on the customizer, not the page) they keep their natural height and the body
   scrolls instead of letting them run into each other (the header and footer stay). In a narrow
   window the available actions go below the toolbar's lists. */
${c("body")} {
  display: grid; grid-template-columns: minmax(0, 1fr) minmax(220px, 280px); gap: ${T.lg};
  flex: 1; min-height: 0;
}
@container ${CUSTOMIZER_CONTAINER} (max-width: ${CUSTOMIZER_NARROW}) { ${c("body")} { grid-template-columns: minmax(0, 1fr); } }
${c("areas")} { display: flex; flex-direction: column; gap: ${T.md}; min-width: 0; min-height: 0; }
${c("area")}[data-area="more"] { flex: 1; }
${c("area")} > ${c("list")}[data-orientation="vertical"] { flex: 1; min-height: calc(3 * ${T.control}); }
${c("area")}, ${c("palette")} { display: flex; flex-direction: column; gap: ${T.xs}; min-width: 0; min-height: 0; }
@container ${CUSTOMIZER_CONTAINER} (max-height: ${CUSTOMIZER_SHORT}) {
  ${c("body")} { overflow: auto; }
  ${c("areas")}, ${c("area")}, ${c("palette")} { min-height: auto; }
  ${c("list")}[data-orientation="vertical"] { flex: none; max-height: calc(6 * ${T.control}); }
}
${c("heading")} {
  margin: 0; font-size: ${T.label}; font-weight: ${T.semibold}; letter-spacing: 0.04em;
  text-transform: uppercase; color: ${T.muted};
}`;

const LISTS = `${c("list")} {
  list-style: none; margin: 0; padding: ${T.xs}; display: flex; gap: ${T.xxs};
  border: 1px dashed ${T.border}; border-radius: ${T.rMd}; background-color: ${T.subtle};
  min-height: calc(${T.control} + 2 * ${T.xs});
}
${c("list")}[data-orientation="horizontal"] { flex-wrap: wrap; align-items: center; }
${c("list")}[data-orientation="vertical"] { flex-direction: column; overflow: auto; }
${c("list")}[data-list="palette"] { flex: 1; min-height: 160px; border-style: solid; background-color: ${T.bg}; }
${c("list")}[data-drop="end"] { box-shadow: inset 0 -2px 0 ${T.accent}; }
${c("list")}[data-orientation="horizontal"][data-drop="end"] { box-shadow: inset -2px 0 0 ${T.accent}; }
${c("item")} {
  display: flex; align-items: center; gap: ${T.xs}; min-height: ${T.control};
  padding: 0 ${T.sm} 0 ${T.xs}; border: 1px solid ${T.border}; border-radius: ${T.rSm};
  background-color: ${T.raised}; cursor: grab; user-select: none;
  transition: background-color ${T.fast} ${T.ease};
}
${c("item")}:hover { background-color: ${T.accentSoft}; }
${c("item")}:focus-visible { ${focusRing} }
${c("item")}:active { cursor: grabbing; }
${c("item")}[data-state="selected"] {
  background-color: ${T.selection}; color: ${T.selectionFg}; border-color: ${T.accent};
}
${c("item")}[data-drop="before"] { box-shadow: inset 0 2px 0 ${T.accent}; }
${c("list")}[data-orientation="horizontal"] > ${c("item")}[data-drop="before"] { box-shadow: inset 2px 0 0 ${T.accent}; }
${c("item")}[data-kind="separator"], ${c("item")}[data-kind="spacer"], ${c("item")}[data-kind="header"] {
  color: ${T.muted}; font-style: italic;
}
${c("item")}[data-state="selected"][data-kind] { color: ${T.selectionFg}; }
${c("item")}[data-kind="separator"] ${c("item.label")}, ${c("item")}[data-kind="spacer"] ${c("item.label")},
${c("item")}[data-kind="header"] ${c("item.label")} {
  /* Room for the italic glyphs' overhang, which the clipping label would cut off. */
  padding-inline-end: 0.2em;
}
${c("item.icon")} { width: ${T.icon}; height: ${T.icon}; flex: none; }
${c("item.label")} { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
${c("list")}[data-orientation="vertical"] ${c("item.label")} { flex: 1; }
${c("item.badge")} {
  flex: none; padding: 0 ${T.xs}; border-radius: ${T.pill}; font-size: ${T.label};
  border: 1px solid ${T.border}; color: ${T.muted};
}
${c("item")}[data-state="selected"] ${c("item.badge")} { color: inherit; border-color: currentColor; }
${c("empty")} { padding: ${T.xs} ${T.sm}; color: ${T.muted}; font-size: ${T.small}; }
${c("palette")}[data-drop="remove"] ${c("list")} { border-color: ${T.danger}; border-style: dashed; }`;

const CONTROLS = `${c("commands")} { display: flex; flex-wrap: wrap; gap: ${T.xs}; }
${c("status")} {
  display: flex; align-items: center; gap: ${T.sm}; margin: 0; min-height: 1.5em; font-size: ${T.small};
  color: ${T.muted};
}
${c("status")}[data-tone="error"] { color: ${T.danger}; }
${c("footer")} { display: flex; flex-wrap: wrap; align-items: center; gap: ${T.xs}; }
${c("footer.spacer")} { flex: 1; }
${p("toolbar")}[data-customizing] { outline: 2px dashed ${T.accent}; outline-offset: -2px; }`;

const SETTINGS = `${p(SETTINGS_PART)} { display: flex; flex-direction: column; gap: ${T.sm}; }
${p("toolbar-customizer-settings.title")} { margin: 0; font-weight: ${T.semibold}; }
${p("toolbar-customizer-settings.intro")}, ${p("toolbar-customizer-settings.empty")} {
  margin: 0; color: ${T.muted}; font-size: ${T.small};
}
${p("toolbar-customizer-settings.list")} { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; }
${p("toolbar-customizer-settings.item")} {
  display: flex; align-items: center; gap: ${T.sm}; padding: ${T.xs} 0;
}
${p("toolbar-customizer-settings.item")} + ${p("toolbar-customizer-settings.item")} { border-top: 1px solid ${T.border}; }
${p("toolbar-customizer-settings.name")} { flex: 1; min-width: 0; }
${p("toolbar-customizer-settings.count")} { color: ${T.muted}; font-size: ${T.small}; }`;

/** The customizer's skin, for the theme compiler (`../../theme/compile.ts`). */
export const TOOLBAR_CUSTOMIZER_SKIN_CSS = [FRAME, LISTS, CONTROLS, SETTINGS].join("\n");
