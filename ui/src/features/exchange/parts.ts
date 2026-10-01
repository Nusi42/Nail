/**
 * Structure of the Exchange parts every theme needs (like `GMAIL_BASE_CSS`):
 * lists without bullets, swatches that show their colour, tabs in a row.
 * Category colours are data (theme geometry); everything else comes from
 * the theme.
 *
 * The integrator appends this to the compiled theme's base CSS
 * (`ui/src/theme/compile.ts`, next to `GMAIL_BASE_CSS`).
 */

/** Lists that show no bullets and no indentation. */
const PLAIN_LISTS = [
  "exchange-categories.items",
  "exchange-mailboxes.items",
  "exchange.categories",
  "focused-tabs.list",
  "exchange.rooms.items",
  "exchange.room.features",
  "exchange.room.busy",
  "exchange.rule-badges",
  "exchange-rules.server.items",
  "exchange.status.warnings",
  "exchange.colors",
];

const sel = (parts: readonly string[]) => parts.map((p) => `[data-part="${p}"]`).join(", ");

export const EXCHANGE_BASE_CSS = `${sel(PLAIN_LISTS)} { list-style: none; margin: 0; padding: 0; }
[data-part="focused-tabs.list"], [data-part="exchange.categories"], [data-part="exchange.rule-badges"], [data-part="exchange.room.features"] { display: flex; flex-wrap: wrap; }
[data-part="exchange.swatch"], [data-part="exchange.chip.swatch"] { display: inline-block; }
[data-part="exchange.colors"] { display: flex; flex-wrap: wrap; margin: 0; min-width: 0; }
`;
