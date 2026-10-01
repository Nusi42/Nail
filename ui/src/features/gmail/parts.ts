/**
 * Structure of the Gmail parts every theme needs (like `PIM_BASE_CSS`):
 * lists without bullets, swatches that show their colour. Colours of
 * labels are data (theme geometry); everything else comes from the theme.
 *
 * The integrator appends this to the compiled theme's base CSS
 * (`ui/src/theme/compile.ts`, next to `PIM_BASE_CSS`).
 */

/** Lists that show no bullets and no indentation. */
const PLAIN_LISTS = ["label-list.items", "category-tabs.list", "gmail.labels"];

const sel = (parts: readonly string[]) => parts.map((p) => `[data-part="${p}"]`).join(", ");

export const GMAIL_BASE_CSS = `${sel(PLAIN_LISTS)} { list-style: none; margin: 0; padding: 0; }
[data-part="category-tabs.list"], [data-part="gmail.labels"] { display: flex; flex-wrap: wrap; }
[data-part="gmail.swatch"], [data-part="gmail.chip.swatch"] { display: inline-block; }
[data-part="gmail.colors"], [data-part="gmail.apply.labels"] { margin: 0; min-width: 0; }
`;
