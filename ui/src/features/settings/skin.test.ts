/**
 * The settings page's skin (M5b WP163): tokens only, emitted after the
 * shared skin and before the theme's rules, the page parts it promises,
 * and the M5a review fixes for the Density control.
 */
import type { ResolvedTheme } from "../../api/generated";
import { defaultTheme } from "../../api/mockData";
import { compileTheme } from "../../theme/compile";
import { SKIN_CSS } from "../../theme/skin";
import { SETTINGS_SKIN_CSS } from "./skin";

/** The CSS blocks as [selectors, body]. */
const blocks = () =>
  SETTINGS_SKIN_CSS.split("}")
    .map((block) => block.split("{"))
    .map(([selectors = "", body = ""]) => [selectors.replace(/@media[^{]*$/, "").trim(), body]);

describe("settings page skin", () => {
  it("sits after the shared skin and before the theme's part rules", () => {
    const theme: ResolvedTheme = {
      ...defaultTheme(),
      parts: [
        {
          selector: "settings-page.nav.item",
          states: [],
          conditions: [],
          ancestors: [],
          declarations: { color: "red" },
          source: null,
        },
      ],
    };
    const css = compileTheme(theme).css;
    const shared = css.indexOf(SKIN_CSS);
    const own = css.indexOf(SETTINGS_SKIN_CSS);
    const rule = css.indexOf('[data-part="settings-page.nav.item"] {\n  color: red;');
    expect(shared).toBeGreaterThan(0);
    expect(own).toBeGreaterThan(shared);
    expect(rule).toBeGreaterThan(own);
  });

  it("styles the page's parts", () => {
    for (const part of [
      "settings-page",
      "settings-page.nav",
      "settings-page.search",
      "settings-page.search.input",
      "settings-page.search.clear",
      "settings-page.group.label",
      "settings-page.nav.item",
      "settings-page.result.section",
      "settings-page.result.setting",
      "settings-page.result.label",
      "settings-page.result-count",
      "settings-page.mark",
      "settings-page.content",
      "settings-page.title",
      "settings-page.body",
      "settings-page.card",
    ]) {
      expect(SETTINGS_SKIN_CSS).toContain(`[data-part="${part}"]`);
    }
    expect(SETTINGS_SKIN_CSS).toContain("[data-settings-found]");
  });

  it("takes colours only from tokens (no system colours)", () => {
    const withoutFallbacks = SETTINGS_SKIN_CSS.replace(
      /var\(--nail-[a-z0-9-]+, #[0-9a-f]{3,8}\)/gi,
      "",
    );
    expect(withoutFallbacks).not.toMatch(/#[0-9a-f]{3,8}\b/i);
    expect(SETTINGS_SKIN_CSS).not.toMatch(/url\(|@import|rgb\(|hsl\(/);
    // The forced-colours rules live in the shared skin.
    expect(SETTINGS_SKIN_CSS).not.toMatch(/Highlight|CanvasText/);
  });

  it("puts a state on every selector of a group, not only the last one", () => {
    const states = [":hover", ":focus-visible", ":focus-within", ":disabled", ":active"];
    for (const [selectors = ""] of blocks()) {
      const items = selectors.split(",").map((item) => item.trim());
      if (items.length < 2) continue;
      for (const state of states) {
        if (items.some((item) => item.includes(state))) {
          expect(
            items.filter((item) => /^\[data-part="[^"]+"\]$/.test(item)),
            `${state} in ${selectors}`,
          ).toEqual([]);
        }
      }
    }
  });

  it("lays out card rows with the label on the left and the control on the right", () => {
    expect(SETTINGS_SKIN_CSS).toMatch(
      /\[data-part="settings-page.card"\] \[data-part="form.field"\] \{[^}]*grid-template-columns: minmax\(0, 1fr\) auto/,
    );
    expect(SETTINGS_SKIN_CSS).toMatch(
      /\[data-part="form.field"\] > \[data-part="form.input"\][^{]*\{[^}]*grid-column: 2/,
    );
  });

  it("keeps the Density control at its natural width and fills its selection in High contrast", () => {
    expect(SETTINGS_SKIN_CSS).toMatch(
      /\[data-part="appearance"\] \[data-part="segmented"\][^{]*\{[^}]*justify-self: start;[^}]*width: max-content/,
    );
    // The High contrast fill is app-wide (the shared skin), for radios and toggles.
    expect(SKIN_CSS).toMatch(
      /:root\[data-nail-mode="high-contrast"\] \[data-part="segmented.option"\]\[aria-checked="true"\], :root\[data-nail-mode="high-contrast"\] \[data-part="segmented.option"\]\[aria-pressed="true"\] \{[^}]*background-color: var\(--nail-color-accent\);[^}]*outline: 2px solid/,
    );
    expect(SKIN_CSS).toMatch(/@media \(forced-colors: active\)[^}]*background-color: Highlight/);
  });
});
