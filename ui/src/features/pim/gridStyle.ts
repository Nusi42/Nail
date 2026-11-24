/**
 * R7 style slot of the time grid package (P1): event cards (short one-line cards, icons, hatched free time, cascaded overlaps), today circle, now line with dot, faint line and gutter label, drag-to-create ghost and inline title form, off-hours and weekend shading, week number corner, tasks in the all-day row.
 *
 * `GRID_BASE_CSS` holds structure every theme needs (appended to
 * `PIM_BASE_CSS`, like `parts.ts`); `GRID_SKIN_CSS` the look on tokens
 * only (appended to `PIM_SKIN_CSS`, like `skin.ts`). Only that package
 * edits this file during R7 (ADR 0246), so the packages never touch the
 * same stylesheet; the integrator may fold it into `parts.ts` / `skin.ts`.
 */

/** Structure of the package's parts (selectors through `[data-part]` only). */
export const GRID_BASE_CSS = "";

/** Look of the package's parts (tokens only, `var(--nail-…)`). */
export const GRID_SKIN_CSS = "";
