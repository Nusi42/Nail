/**
 * Responsive rules shared by the windows and dialogs outside the mail and
 * PIM views (P1 WP191, ADR 0139): form rows that wrap with aligned labels,
 * button rows that wrap, cards and window bodies that scroll their own
 * content and grow with the window. Token schema only (`var(--nail-…)`) and
 * `[data-part]` selectors only, like `./skin.ts`.
 *
 * The compiler emits it after the shared skin and every feature skin and
 * before the theme's part rules: it may adjust the primitives of
 * `./skin.ts` (buttons, inputs, cards) for narrow windows, and any
 * `theme.kdl` still overrides it.
 */
export const RESPONSIVE_SKIN_CSS = `/* P1 responsive rules (WP191) */`;
