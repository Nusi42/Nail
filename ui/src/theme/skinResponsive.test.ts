import type { ResolvedTheme } from "../api/generated";
import { defaultTheme } from "../api/mockData";
import { APPEARANCE_SKIN_CSS } from "../features/appearance/skin";
import { compileTheme } from "./compile";
import { SKIN_CSS } from "./skin";
import { RESPONSIVE_SKIN_CSS } from "./skinResponsive";

describe("P1 responsive skin (theme/skinResponsive.ts)", () => {
  it("comes after the shared and feature skins and before the theme's part rules", () => {
    const theme: ResolvedTheme = {
      ...defaultTheme(),
      parts: [
        {
          selector: "form.row",
          states: [],
          conditions: [],
          ancestors: [],
          declarations: { color: "red" },
          source: null,
        },
      ],
    };
    const css = compileTheme(theme).css;
    const skin = css.indexOf(SKIN_CSS);
    const feature = css.indexOf(APPEARANCE_SKIN_CSS);
    const responsive = css.indexOf(RESPONSIVE_SKIN_CSS);
    const rule = css.indexOf('[data-part="form.row"] {\n  color: red;');
    expect(skin).toBeGreaterThanOrEqual(0);
    expect(feature).toBeGreaterThan(skin);
    expect(responsive).toBeGreaterThan(feature);
    expect(rule).toBeGreaterThan(responsive);
  });

  it("uses tokens and data-part selectors only", () => {
    expect(RESPONSIVE_SKIN_CSS.match(/#[0-9a-f]{3,8}\b/gi) ?? []).toEqual([]);
    expect(RESPONSIVE_SKIN_CSS).not.toMatch(/(^|\s)\.[a-z]/m);
    expect(RESPONSIVE_SKIN_CSS.split("{").length).toBe(RESPONSIVE_SKIN_CSS.split("}").length);
  });
});
