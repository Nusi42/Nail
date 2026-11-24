/**
 * R7 style slot of the quick look and editor package (P2): the quick look popover (`pim.popover.*`), the compact event editor, the date / time input, recurrence and reminder pieces, the reminder toast.
 *
 * `LOOK_BASE_CSS` holds structure every theme needs (appended to
 * `PIM_BASE_CSS`, like `parts.ts`); `LOOK_SKIN_CSS` the look on tokens
 * only (appended to `PIM_SKIN_CSS`, like `skin.ts`). Only that package
 * edits this file during R7 (ADR 0246), so the packages never touch the
 * same stylesheet; the integrator may fold it into `parts.ts` / `skin.ts`.
 */

/** Structure of the package's parts (selectors through `[data-part]` only). */
export const LOOK_BASE_CSS = "";

/** Look of the package's parts (tokens only, `var(--nail-…)`). */
export const LOOK_SKIN_CSS = "";
