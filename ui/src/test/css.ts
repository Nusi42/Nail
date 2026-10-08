/** Helpers for tests of CSS-in-TS (skins, base CSS). */

/**
 * Every top-level `{ … }` block of a stylesheet text with what precedes it;
 * fails the test unless the braces balance and nothing follows the last
 * block.
 */
export function cssBlocks(css: string): string[] {
  const out: string[] = [];
  let depth = 0;
  let start = 0;
  for (let i = 0; i < css.length; i++) {
    if (css[i] === "{") depth++;
    if (css[i] === "}") {
      depth--;
      if (depth === 0) {
        out.push(css.slice(start, i + 1));
        start = i + 1;
      }
    }
    expect(depth).toBeGreaterThanOrEqual(0);
  }
  expect(depth).toBe(0);
  expect(css.slice(start).trim()).toBe("");
  return out;
}

/** Inserts each rule into a fresh sheet: none may be refused or dropped. */
export function expectInsertable(rules: readonly string[]): void {
  const sheet = new CSSStyleSheet();
  for (const rule of rules) {
    expect(() => sheet.insertRule(rule, sheet.cssRules.length), rule).not.toThrow();
  }
  expect(sheet.cssRules.length).toBe(rules.length);
}
