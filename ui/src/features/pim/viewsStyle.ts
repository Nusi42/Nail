/**
 * R7 style slot of the views and shell package (P3): toolbar search and results, quick-add field and preview chips, zone chips, the narrow-window drawer and floating New event, agenda time column and now marker, month spanning bars, dot lines and today pill, mini-month band, dots and week numbers, year heat shading, free-time axis, empty states, the Due this week panel, the calendar list and settings.
 *
 * `VIEWS_BASE_CSS` holds structure every theme needs (appended to
 * `PIM_BASE_CSS`, like `parts.ts`); `VIEWS_SKIN_CSS` the look on tokens
 * only (appended to `PIM_SKIN_CSS`, like `skin.ts`). Only that package
 * edits this file during R7 (ADR 0246), so the packages never touch the
 * same stylesheet; the integrator may fold it into `parts.ts` / `skin.ts`.
 */

/** Structure of the package's parts (selectors through `[data-part]` only). */
export const VIEWS_BASE_CSS = "";

/** Look of the package's parts (tokens only, `var(--nail-…)`). */
export const VIEWS_SKIN_CSS = "";
