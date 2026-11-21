import { p } from "../../theme/selectors";
import { T } from "../../theme/tokens";
import { SETTINGS_BODY_END, SETTINGS_SKIN_CSS } from "../settings/skin";
import { VISUALLY_HIDDEN } from "../../theme/declarations";
import { RULES_CONTAINER, RULES_SKIN_CSS, RULES_STACKED, repeatedRowLabels } from "./skin";

describe("the rules skin (R2)", () => {
  it("hides a row's labels only under a row of the same kind", () => {
    const conditions = document.createElement("ul");
    conditions.innerHTML = ["address", "subject", "subject"]
      .map(
        (kind) =>
          `<li data-part="rules.group.item"><div data-part="rules.condition" data-kind="${kind}"><label data-part="form.label">L</label></div></li>`,
      )
      .join("");
    const actions = document.createElement("ol");
    actions.innerHTML = ["move", "tag", "tag"]
      .map(
        (kind) =>
          `<li data-part="rules.action" data-kind="${kind}"><label data-part="form.label">L</label></li>`,
      )
      .join("");
    document.body.append(conditions, actions);
    const repeated = new Set(
      repeatedRowLabels().flatMap((selector) => [...document.querySelectorAll(selector)]),
    );
    // The skin hides exactly those labels.
    expect(RULES_SKIN_CSS).toContain(`${repeatedRowLabels().join(",\n")} { ${VISUALLY_HIDDEN} }`);
    const hidden = (root: HTMLElement) =>
      [...root.querySelectorAll('[data-part="form.label"]')].map((label) => repeated.has(label));
    // The second row (another kind) keeps its labels, the third (same kind) not.
    expect(hidden(conditions)).toEqual([false, false, true]);
    expect(hidden(actions)).toEqual([false, false, true]);
    conditions.remove();
    actions.remove();
  });

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
    // In Settings it sticks too, to the page's bottom edge over its padding (never static).
    expect(RULES_SKIN_CSS).not.toMatch(/rules\.footer"\] \{ position: static/);
    expect(RULES_SKIN_CSS).toContain(
      `bottom: calc(-1 * var(${SETTINGS_BODY_END}, 0px)); padding-bottom: var(${SETTINGS_BODY_END}, 0px);`,
    );
    expect(SETTINGS_SKIN_CSS).toContain(`padding: ${T.xs} ${T.xxl} var(${SETTINGS_BODY_END});`);
    expect(RULES_SKIN_CSS).toContain(`@container ${RULES_CONTAINER} (max-width: ${RULES_STACKED})`);
    // "Saved" fades, unless the user asks for less motion.
    expect(RULES_SKIN_CSS).toMatch(/prefers-reduced-motion: reduce[^}]*animation: none/);
  });
});
