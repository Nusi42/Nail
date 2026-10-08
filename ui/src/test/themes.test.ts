import { describe, expect, it } from "vitest";
import { BUILTIN_THEMES } from "../api/generated";
import { THEME_DENSITY_CASES, builtinTheme } from "./themes";

describe("the theme matrix of the accessibility tests", () => {
  it("covers the five built-in themes", () => {
    expect(BUILTIN_THEMES.map(([id]) => id)).toEqual([
      "frost",
      "sage",
      "graphite",
      "nocturne",
      "high-contrast",
    ]);
  });

  it("has every theme in both densities", () => {
    expect(THEME_DENSITY_CASES).toHaveLength(BUILTIN_THEMES.length * 2);
    expect(THEME_DENSITY_CASES[1]).toEqual(["frost", "compact"]);
  });

  it("resolves a built-in theme in its density", () => {
    expect(builtinTheme("graphite", "compact").density).toBe("compact");
    expect(builtinTheme("frost").density).toBe("comfortable");
  });
});
