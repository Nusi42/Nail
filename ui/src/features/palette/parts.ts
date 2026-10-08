/**
 * Structure of the palette's parts every theme needs: the list scrolls
 * inside the window, rows are one line with the title first and the key
 * bindings at the end. Colours (the active row, dimmed unavailable rows,
 * the highlighted letters) come from `theme.kdl` through
 * `[data-state~=active]`, `[data-state~=unavailable]` and `palette.match`.
 *
 * The integrator appends this to the compiled theme's base CSS
 * (`ui/src/theme/compile.ts`).
 */
import { INLINE_GAP } from "../../theme/declarations";
export const PALETTE_BASE_CSS = `[data-part="palette"] { display: flex; flex-direction: column; min-height: 0; }
[data-part="palette.input"] { width: 100%; box-sizing: border-box; }
[data-part="palette.list"] { list-style: none; margin: 0; padding: 0; overflow-y: auto; max-height: 60vh; }
[data-part="palette.item"] { display: flex; align-items: center; gap: ${INLINE_GAP}; cursor: default; }
[data-part="palette.item.title"] { flex: 1 1 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
[data-part="palette.shortcuts"] { display: flex; gap: 0.25em; flex: 0 0 auto; }
[data-part="palette.match"] { background: none; color: inherit; font-weight: bold; }
[data-part="palette.footer"] { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; }
`;
