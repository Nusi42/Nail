/**
 * The look and structure of the window manager (M5c WP171, ADR 0116), on
 * the token schema only (`var(--nail-…)`), like `../../theme/skin.ts`. The
 * compiler emits it after the shared skin and before the theme's part
 * rules, so any `theme.kdl` still overrides it. Where a window is and how
 * big it is comes from its geometry rule (`[data-geom]`), never from here.
 *
 * Parts:
 * - `window-layer` (`data-modal` while a modal window is open): fills the
 *   page and lets the pointer through to the main window around the windows
 * - `window-scrim`: over the main window while a modal window blocks it
 *   (below every window: the other windows stay usable)
 * - `window` (`data-state` = `active` / `maximized` / `snapped`,
 *   `data-snap` = `left` / `right`, `data-modal`, `data-owner` (what a
 *   modal window blocks: `main` or a window id), `data-blocked` (its
 *   confirmation is open: dimmed, not usable), `data-mode` = `move` /
 *   `size` (Alt+F7 / Alt+F8), `data-dragging` = `move` / `resize`)
 * - `window.header` (the title bar: drag to move, double-click to
 *   maximize, arrow keys when focused), `window.title`, `window.maximize`
 *   (+ `.icon`), `window.close` (styled by the shared skin), `window.hint`
 *   and `window.status` (for assistive technology only)
 * - `window.handle` (`data-edge` = `n` `e` `s` `w` `ne` `se` `sw` `nw`)
 * - `window-snap-preview` (`data-snap` = `left` / `right`): the half a
 *   dragged window will fill when dropped
 */
import { p } from "../../theme/selectors";
import { QUIET_HOVER, VISUALLY_HIDDEN } from "../../theme/declarations";
import { T } from "../../theme/tokens";

/** The grab width of an edge handle and the size of a corner handle. */
const EDGE = "6px";
const CORNER = "14px";

const LAYER = `${p("window-layer")} {
  position: fixed; inset: 0; z-index: 100; display: block;
  padding: 0; background: none; pointer-events: none;
}
${p("window-scrim")} { position: absolute; inset: 0; pointer-events: auto; background-color: ${T.scrim}; }`;

const FRAME = `${p("window-layer")} > ${p("window")} {
  position: absolute; display: flex; flex-direction: column; box-sizing: border-box;
  overflow: hidden; pointer-events: auto;
}
${p("window-layer")} > ${p("window")}:not([data-state~="active"]) { display: flex; }
${p("window-layer")} > ${p("window")}[data-state~="active"] { border-color: ${T.accent}; }
${p("window-layer")} > ${p("window")}[data-state~="maximized"],
${p("window-layer")} > ${p("window")}[data-state~="snapped"] { border-radius: 0; }
${p("window-layer")} > ${p("window")}[data-blocked]::after {
  content: ""; position: absolute; inset: 0; z-index: 5; background-color: ${T.scrim};
}
${p("window-layer")} > ${p("window")}[data-dragging] { user-select: none; }
${p("window-layer")} > ${p("window")}[data-dragging] ${p("window.body")} { pointer-events: none; }
${p("window-layer")} > ${p("window")}[data-dragging="move"] ${p("window.header")} { cursor: grabbing; }`;

const TITLE_BAR = `${p("window-layer")} ${p("window.header")} {
  position: relative; z-index: 2; user-select: none; touch-action: none; cursor: default;
}
${p("window-layer")} ${p("window.header")}:focus-visible { outline: 2px solid ${T.focus}; outline-offset: -2px; }
${p("window")}[data-mode] ${p("window.header")} { outline: 2px dashed ${T.accent}; outline-offset: -2px; }
${p("window.title")} { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
${p("window.hint")}, ${p("window.status")} { ${VISUALLY_HIDDEN} }
${p("window.maximize")} {
  display: inline-flex; align-items: center; justify-content: center; flex: 0 0 auto;
  width: ${T.control}; height: ${T.control}; padding: 0;
  border: none; border-radius: ${T.rSm}; color: ${T.muted}; background-color: transparent;
}
${p("window.maximize")}:hover { ${QUIET_HOVER} }
${p("window.maximize")}:focus-visible { outline: 2px solid ${T.focus}; outline-offset: 2px; }
${p("window.maximize.icon")} { width: ${T.icon}; height: ${T.icon}; }`;

const edge = (name: string, rules: string) =>
  `${p("window.handle")}[data-edge="${name}"] { ${rules} }`;

/** An edge handle: along the side `at`, between the corners, `EDGE` deep. */
const side = (
  name: string,
  at: string,
  along: readonly [string, string],
  depth: string,
  cursor: string,
) =>
  edge(
    name,
    `${at}: 0; ${along[0]}: ${CORNER}; ${along[1]}: ${CORNER}; ${depth}: ${EDGE}; cursor: ${cursor};`,
  );

/** A corner handle: `CORNER` square at the corner of `vertical` and `horizontal`. */
const corner = (name: string, vertical: string, horizontal: string, cursor: string) =>
  edge(
    name,
    `${vertical}: 0; ${horizontal}: 0; width: ${CORNER}; height: ${CORNER}; cursor: ${cursor};`,
  );

const ACROSS = ["left", "right"] as const;
const DOWN = ["top", "bottom"] as const;

const HANDLES = [
  `${p("window.handle")} { position: absolute; z-index: 3; touch-action: none; }`,
  side("n", "top", ACROSS, "height", "ns-resize"),
  side("s", "bottom", ACROSS, "height", "ns-resize"),
  side("e", "right", DOWN, "width", "ew-resize"),
  side("w", "left", DOWN, "width", "ew-resize"),
  corner("ne", "top", "right", "nesw-resize"),
  corner("sw", "bottom", "left", "nesw-resize"),
  corner("nw", "top", "left", "nwse-resize"),
  corner("se", "bottom", "right", "nwse-resize"),
].join("\n");

const SNAP = `${p("window-snap-preview")} {
  position: absolute; top: ${T.sm}; bottom: ${T.sm}; width: calc(50% - 1.5 * ${T.sm});
  z-index: 10000; pointer-events: none; box-sizing: border-box;
  border: 2px dashed ${T.accent}; border-radius: ${T.rLg};
  background-color: color-mix(in srgb, ${T.accent} 12%, transparent);
}
${p("window-snap-preview")}[data-snap="left"] { left: ${T.sm}; }
${p("window-snap-preview")}[data-snap="right"] { right: ${T.sm}; }`;

export const WINDOWS_SKIN_CSS = [LAYER, FRAME, TITLE_BAR, HANDLES, SNAP].join("\n");

/** Every part this skin styles (the theme reference lists them). */
export const WINDOW_PARTS: readonly string[] = [
  ...new Set([...WINDOWS_SKIN_CSS.matchAll(/\[data-part="([^"]+)"\]/g)].map((m) => m[1] ?? "")),
];
