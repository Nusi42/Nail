import { p } from "../../theme/selectors";
import { RULES_CONTAINER, RULES_SKIN_CSS, RULES_STACKED } from "./skin";

describe("the rules skin (R2)", () => {
  it("styles the window by part only, on tokens", () => {
    for (const part of [
      "rules",
      "rules.panes",
      "rules.footer",
      "rules.confirm",
      "rules.text.area",
      "rules.text.highlight",
      "rules.text.source",
      "rules.issues.items",
      "exchange.rule-badge.reasons",
    ]) {
      expect(RULES_SKIN_CSS).toContain(p(part));
    }
    // Token values only, no class names.
    expect(RULES_SKIN_CSS).not.toMatch(/(^|\s)\.[a-z]/);
    expect(RULES_SKIN_CSS).toContain("var(--nail-font-mono)");
  });

  it("keeps the footer in view and stacks the list below the narrow width", () => {
    expect(RULES_SKIN_CSS).toContain(`container: ${RULES_CONTAINER} / inline-size`);
    expect(RULES_SKIN_CSS).toMatch(/\[data-part="rules\.footer"\] \{ position: sticky; bottom: 0;/);
    expect(RULES_SKIN_CSS).toContain(`@container ${RULES_CONTAINER} (max-width: ${RULES_STACKED})`);
    // "Saved" fades, unless the user asks for less motion.
    expect(RULES_SKIN_CSS).toMatch(/prefers-reduced-motion: reduce[^}]*animation: none/);
  });
});
