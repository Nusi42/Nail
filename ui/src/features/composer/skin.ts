/**
 * The composer's window-size reflow (M5c WP172, ADR 0116), on the token
 * schema only (`var(--nail-…)`), like `../../theme/skin.ts`. The theme
 * compiler emits it after the shared skin and before the theme's part
 * rules (integration: `theme/compile.ts`), so `theme.kdl` keeps the look.
 *
 * Several composers can be open at once and each window can be resized
 * down to its minimum size (520 × 400 by default), so the composer adapts
 * to its own width, not the page's — a container query on the composer:
 *
 * - Always: the footer (`composer.footer`: the attachment reminder, an
 *   error, the send time and `composer.actions` with Send, Save, Discard)
 *   sticks to the bottom of the composer, which scrolls when the window is
 *   short, so Send and what it needs stay in view and nothing about sending
 *   scrolls under the footer; the formatting bar, the actions and the
 *   send-later row wrap instead of overflowing. The rows never shrink below
 *   their content (only the message text gives way, down to its minimum
 *   height), so a short window scrolls instead of squeezing them.
 * - Narrower than 36rem: labels go above their fields, the Markdown preview
 *   goes below the source, the editors start lower.
 * - Narrower than 26rem: the footer's spacer goes, so its buttons wrap as
 *   one group, and the saved state takes its own line.
 *
 * The responsive rules are scoped inside the composer (two-part selectors)
 * so they win over the theme's single-part rules only where the width asks
 * for it.
 */

/** `[data-part="…"]` for one part. */
const p = (part: string) => `[data-part="${part}"]`;
/** A part inside the composer. */
const inComposer = (part: string) => `${p("composer")} ${p(part)}`;

/** Token shorthands. */
const T = {
  bg: "var(--nail-color-bg)",
  border: "var(--nail-color-border)",
  xxs: "var(--nail-space-xxs)",
  xs: "var(--nail-space-xs)",
  sm: "var(--nail-space-sm)",
  md: "var(--nail-space-md)",
} as const;

/** The container the queries below measure. */
export const COMPOSER_CONTAINER = "nail-composer";
/** Below this width labels go above their fields. */
export const COMPOSER_NARROW = "36rem";
/** Below this width the footer wraps as one group. */
export const COMPOSER_TIGHT = "26rem";

const BASE = `${p("composer")} { container: ${COMPOSER_CONTAINER} / inline-size; }
${p("composer")} > :not(${p("composer.body")}) { flex-shrink: 0; }
${p("composer")} > ${p("composer.footer")} {
  position: sticky; bottom: calc(-1 * ${T.md}); z-index: 1; display: flex; flex-direction: column; gap: ${T.sm};
  margin-top: auto; padding-block: ${T.sm} ${T.md}; border-top: 1px solid ${T.border}; background-color: ${T.bg};
}
${inComposer("composer.actions")} { flex-wrap: wrap; }
${inComposer("composer.toolbar")} { flex-wrap: wrap; min-width: 0; }
${inComposer("composer.send-later")} { flex-wrap: wrap; }
${inComposer("composer.attachments.list")} { display: flex; flex-wrap: wrap; gap: ${T.xs}; min-width: 0; }
${inComposer("composer.field.input")}, ${inComposer("composer.identity")} { min-width: 0; max-width: 100%; }
${p("composer.confirm")} { display: grid; gap: ${T.md}; align-content: start; }
${p("composer.confirm.question")} { margin: 0; overflow-wrap: anywhere; }
${p("composer.confirm.actions")} { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: ${T.sm}; }`;

const NARROW = `@container ${COMPOSER_CONTAINER} (max-width: ${COMPOSER_NARROW}) {
${inComposer("composer.field")} { grid-template-columns: minmax(0, 1fr); gap: ${T.xxs}; }
${inComposer("composer.markdown")}[data-state~="split"] { grid-template-columns: minmax(0, 1fr); }
${inComposer("composer.body")}, ${inComposer("composer.html.content")}, ${inComposer("composer.html.source")}, ${inComposer("composer.markdown.editor")}, ${inComposer("composer.preview")} { min-height: 9rem; }
}`;

const TIGHT = `@container ${COMPOSER_CONTAINER} (max-width: ${COMPOSER_TIGHT}) {
${inComposer("composer.actions")} > ${p("composer.spacer")} { display: none; }
${inComposer("composer.saved")} { flex-basis: 100%; }
${inComposer("composer.toolbar")} > ${p("composer.spacer")} { display: none; }
}`;

/** The composer skin. */
export const COMPOSER_SKIN_CSS = [BASE, NARROW, TIGHT].join("\n");

/** Parts the skin styles (each one the composer emits; checked in `skin.test.ts`). */
export const COMPOSER_SKIN_PARTS: readonly string[] = [
  ...new Set([...COMPOSER_SKIN_CSS.matchAll(/\[data-part="([^"]+)"\]/g)].map((m) => m[1] ?? "")),
].sort();
