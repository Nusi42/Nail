/**
 * Structure of the confirmation center's parts every theme needs (like
 * `EXCHANGE_BASE_CSS`): lists without bullets, notices pinned to a corner,
 * header chips in a row, before / after tables that wrap long values.
 * Colours (the destructive / outgoing highlight via `data-risk`), spacing
 * and fonts come from `theme.kdl`.
 *
 * The integrator appends this to the compiled theme's base CSS
 * (`ui/src/theme/compile.ts`, next to `EXCHANGE_BASE_CSS`).
 */

/** Lists that show no bullets and no indentation. */
export const AGENTS_PLAIN_LISTS = [
  "confirmation-center.list",
  "confirmation.targets.list",
  "confirmation-notices.list",
  "action-policies.list",
];

const sel = (parts: readonly string[]) => parts.map((p) => `[data-part="${p}"]`).join(", ");

export const AGENTS_BASE_CSS = `${sel(AGENTS_PLAIN_LISTS)} { list-style: none; margin: 0; padding: 0; }
[data-part="confirmation.header"], [data-part="confirmation.buttons"], [data-part="confirmation.times"], [data-part="confirmation.outcome"], [data-part="confirmation-center.views"] { display: flex; flex-wrap: wrap; align-items: center; }
[data-part="confirmation.target.label"], [data-part="confirmation.change.before"], [data-part="confirmation.change.after"], [data-part="confirmation-notice.summary"] { overflow-wrap: anywhere; white-space: pre-wrap; }
[data-part="confirmation.changes"] { border-collapse: collapse; max-width: 100%; }
[data-part="confirmation-notices"] { position: fixed; left: 16px; bottom: 16px; z-index: 310; max-width: min(420px, calc(100% - 32px)); }
[data-part="confirmation-notice"] { display: flex; align-items: flex-start; flex-wrap: wrap; }
[data-part="confirmation-notice.content"] { display: flex; flex-direction: column; flex: 1 1 auto; min-width: 0; }
[data-part="confirmation-indicator"] { display: inline-flex; align-items: center; }
`;
