/**
 * Structure of pop-out windows every theme needs (M5c WP173): the pop-out
 * page fills its OS window — a header (title, "Return to main window") over
 * the window's content, which scrolls on its own — and the title-bar
 * button "Open in new window" sits beside the window's close button.
 *
 * Parts: `pop-out` (`data-window`), `pop-out.header`, `pop-out.title`,
 * `pop-out.error`, `pop-out.return` (+ `.icon`, `.label`), `pop-out.body`;
 * `window.pop-out` (+ `.icon`; `data-state="error"` after a failed try).
 *
 * Colours, fonts and spacing come from the shared skin's `window.*` parts
 * and the theme. The integrator appends this to the compiled theme's base
 * CSS (`ui/src/theme/compile.ts`).
 */
import { p } from "../../theme/selectors";

export const POPOUT_BASE_CSS = `${p("pop-out")} { position: fixed; inset: 0; display: flex; flex-direction: column; min-height: 0; overflow: hidden; }
${p("pop-out.header")} { display: flex; align-items: center; flex: 0 0 auto; gap: var(--nail-space-sm); min-height: calc(var(--nail-size-control) + 3 * var(--nail-space-xs)); padding: 0 var(--nail-space-sm) 0 var(--nail-space-md); border-bottom: 1px solid var(--nail-color-border); background-color: var(--nail-color-bg); }
${p("pop-out.title")} { flex: 1 1 auto; min-width: 0; margin: 0; font: inherit; font-weight: var(--nail-font-weight-semibold); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
${p("pop-out.error")} { margin: 0; color: var(--nail-color-danger); font-size: var(--nail-font-size-small); }
${p("pop-out.return")} { display: inline-flex; align-items: center; gap: var(--nail-space-xs); flex: 0 0 auto; height: var(--nail-size-control); padding: 0 var(--nail-space-sm); border: 1px solid var(--nail-color-border); border-radius: var(--nail-radius-sm); color: var(--nail-color-fg); background-color: transparent; font: inherit; }
${p("pop-out.return")}:hover { background-color: var(--nail-color-bg-raised); }
${p("pop-out.return")}:focus-visible, ${p("window.pop-out")}:focus-visible { outline: 2px solid var(--nail-color-focus-ring); outline-offset: 1px; }
${p("pop-out.return.icon")}, ${p("window.pop-out.icon")} { width: var(--nail-size-icon); height: var(--nail-size-icon); }
${p("pop-out.body")} { display: flex; flex-direction: column; flex: 1 1 auto; min-height: 0; overflow: auto; }
${p("pop-out.body")} > * { flex: 1 1 auto; min-height: 0; }
${p("window.pop-out")} { display: inline-flex; align-items: center; justify-content: center; width: var(--nail-size-control); height: var(--nail-size-control); padding: 0; border: none; border-radius: var(--nail-radius-sm); color: var(--nail-color-fg-muted); background-color: transparent; }
${p("window.pop-out")}:hover { color: var(--nail-color-fg); background-color: var(--nail-color-bg-raised); }
${p("window.pop-out")}[data-state="error"] { color: var(--nail-color-danger); }
`;
