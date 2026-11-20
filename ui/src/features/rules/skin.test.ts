import { p } from "../../theme/selectors";
import { RULES_SKIN_CSS } from "./skin";

describe("the rules skin (R1)", () => {
  it("styles the indicator, its explanation and the run panel by part only", () => {
    for (const part of [
      "rules.issues.badge",
      "rules.issues.details",
      "rules.run-panel",
      "rules.run.outcome",
    ]) {
      expect(RULES_SKIN_CSS).toContain(p(part));
    }
    // Token values only, no class names.
    expect(RULES_SKIN_CSS).not.toMatch(/(^|\s)\.[a-z]/);
    expect(RULES_SKIN_CSS).toContain("var(--nail-color-danger)");
  });
});
