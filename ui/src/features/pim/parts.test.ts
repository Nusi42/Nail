import { PIM_BASE_CSS } from "./parts";

describe("PIM base CSS", () => {
  it("is safe and every rule parses", () => {
    // Nothing that loads content (the theme compiler refuses the same).
    expect(PIM_BASE_CSS).not.toMatch(/url\s*\(|@import|expression\s*\(|javascript:/i);
    const sheet = new CSSStyleSheet();
    const rules = PIM_BASE_CSS.split("\n").filter((line) => line.trim() !== "");
    for (const rule of rules) {
      expect(() => sheet.insertRule(rule, sheet.cssRules.length), rule).not.toThrow();
    }
    expect(sheet.cssRules.length).toBe(rules.length);
    // The time grid's layers are positioned by the base CSS, not by a theme.
    expect(PIM_BASE_CSS).toMatch(/\[data-part="calendar.event"\] \{ position: absolute;/);
    expect(PIM_BASE_CSS).toMatch(
      /\[data-part="calendar.columns"\] \{ position: absolute; inset: 0;/,
    );
  });
});
