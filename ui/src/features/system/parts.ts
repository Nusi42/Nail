/**
 * Structure of the desktop settings' parts every theme needs (like
 * `AGENTS_BASE_CSS`): lists without bullets, button rows that wrap, the
 * composer notice for drafts from other programs as a row, release notes
 * that wrap long words. Colours, spacing and fonts come from `theme.kdl`
 * (status colours via `data-state` / `data-tone`).
 *
 * The integrator appends this to the compiled theme's base CSS
 * (`ui/src/theme/compile.ts`, next to `AGENTS_BASE_CSS`).
 */

/** Lists that show no bullets and no indentation. */
export const SYSTEM_PLAIN_LISTS = ["default-apps.list", "about-nail.facts", "first-run.steps"];

/** Rows of buttons / chips. */
export const SYSTEM_ROWS = [
  "update-status.buttons",
  "default-apps.buttons",
  "first-run.buttons",
  "program-draft",
  "default-apps.item",
];

const sel = (parts: readonly string[]) => parts.map((p) => `[data-part="${p}"]`).join(", ");

export const SYSTEM_BASE_CSS = `${sel(SYSTEM_PLAIN_LISTS)} { list-style: none; margin: 0; padding: 0; }
${sel(SYSTEM_ROWS)} { display: flex; flex-wrap: wrap; align-items: center; }
[data-part="default-apps.item.name"], [data-part="program-draft.text"] { flex: 1 1 auto; min-width: 0; }
[data-part="release-notes"], [data-part="about-nail.fact.value"], [data-part="desktop-settings.endpoint"] { overflow-wrap: anywhere; }
[data-part="update-status.progress"] { width: 100%; }
[data-part="about-nail.fact"] { display: flex; flex-wrap: wrap; }
[data-part="about-nail.fact.label"] { min-width: 12em; }
`;
