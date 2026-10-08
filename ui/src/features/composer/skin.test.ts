/**
 * The composer's window-size reflow (M5c WP172): tokens only, container
 * queries on the composer itself, a footer that stays reachable, and only
 * parts the composer really emits.
 */
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import {
  COMPOSER_CONTAINER,
  COMPOSER_NARROW,
  COMPOSER_SKIN_CSS,
  COMPOSER_SKIN_PARTS,
  COMPOSER_TIGHT,
} from "./skin";
import { cssBlocks } from "../../test/css";

const sources = readdirSync(__dirname)
  .filter((f) => /\.tsx?$/.test(f) && !f.includes(".test.") && f !== "skin.ts")
  .map((f) => readFileSync(join(__dirname, f), "utf8"))
  .join("\n");

describe("composer skin", () => {
  it("is balanced and uses tokens, not literal colours", () => {
    expect(cssBlocks(COMPOSER_SKIN_CSS).length).toBeGreaterThan(5);
    expect(COMPOSER_SKIN_CSS).not.toMatch(/#[0-9a-f]{3,8}\b/i);
    expect(COMPOSER_SKIN_CSS).not.toMatch(/rgb\(|hsl\(/);
    expect(COMPOSER_SKIN_CSS).toContain("var(--nail-color-bg)");
  });

  it("measures the composer, not the page", () => {
    expect(COMPOSER_SKIN_CSS).toContain(
      `[data-part="composer"] { container: ${COMPOSER_CONTAINER} / inline-size; }`,
    );
    expect(COMPOSER_SKIN_CSS).not.toContain("@media");
    const queries = [...COMPOSER_SKIN_CSS.matchAll(/@container (\S+) \(max-width: (\S+)\)/g)];
    expect(queries.map((m) => [m[1], m[2]])).toEqual([
      [COMPOSER_CONTAINER, COMPOSER_NARROW],
      [COMPOSER_CONTAINER, COMPOSER_TIGHT],
    ]);
  });

  it("keeps the footer reachable and reflows fields, editor and footer", () => {
    const css = COMPOSER_SKIN_CSS;
    // The footer (reminder, error, send time, actions) sticks together:
    // nothing about sending scrolls under it (M5c review).
    // P1: it reaches into the composer's bottom padding (and fills it), so no
    // text scrolls by below it.
    expect(css).toMatch(
      /\[data-part="composer"\] > \[data-part="composer\.footer"\] \{[^}]*position: sticky; bottom: calc\(-1 \* var\(--nail-space-md\)\);[^}]*padding-block: var\(--nail-space-sm\) var\(--nail-space-md\);/,
    );
    expect(css).toContain(
      `[data-part="composer"] [data-part="composer.actions"] { flex-wrap: wrap; }`,
    );
    // Rows never shrink into each other (only the message text gives way):
    // a short window scrolls (the high-contrast attachment reminder, M5c review).
    expect(css).toContain(
      `[data-part="composer"] > :not([data-part="composer.body"]) { flex-shrink: 0; }`,
    );
    // P1 review: the rich editor (and its source view) fills the height like the textarea.
    expect(css).toContain(
      `[data-part="composer"] [data-part="composer.html.editor"] > * { flex: 1 1 auto; display: flex; flex-direction: column; min-height: 0; }`,
    );
    expect(css).toContain(
      `[data-part="composer"] [data-part="composer.html.content"], [data-part="composer"] [data-part="composer.html.source"] { flex: 1 1 auto; }`,
    );
    // P1 verify: the To / Cc / Bcc inputs span their row like Subject (not the browser's 20 characters).
    expect(css).toContain(
      `[data-part="composer"] [data-part="composer.address"] > [data-part="composer.field.input"] { box-sizing: border-box; width: 100%; }`,
    );
    const narrow = css.slice(css.indexOf(`(max-width: ${COMPOSER_NARROW})`));
    // Labels above their fields; the Markdown preview below the source.
    expect(narrow).toMatch(/composer\.field"\] \{ grid-template-columns: minmax\(0, 1fr\);/);
    expect(narrow).toMatch(
      /composer\.markdown"\]\[data-state~="split"\] \{ grid-template-columns: minmax\(0, 1fr\); \}/,
    );
    expect(narrow).toMatch(/composer\.body"\][^{]*\{ min-height: 9rem; \}/);
    const tight = css.slice(css.indexOf(`(max-width: ${COMPOSER_TIGHT})`));
    expect(tight).toMatch(
      /composer\.actions"\] > \[data-part="composer\.spacer"\] \{ display: none; \}/,
    );
  });

  it("styles only parts the composer emits", () => {
    expect(COMPOSER_SKIN_PARTS.length).toBeGreaterThan(10);
    for (const part of COMPOSER_SKIN_PARTS) {
      expect({ part, emitted: sources.includes(`"${part}"`) }).toEqual({ part, emitted: true });
    }
  });
});
