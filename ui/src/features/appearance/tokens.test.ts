/**
 * M5d WP183: the theme editor's token model — groups, CSS values, colour
 * picker values, reference resolution for the live preview, part entries
 * and selector checks.
 */
import { describe, expect, it } from "vitest";
import type { ThemePartRule, ThemeTokenValue } from "../../api/generated";
import { previewStyles } from "./ThemePreview";
import {
  TOKEN_GROUPS,
  cssValue,
  draftKey,
  hasModeValue,
  isPartSelector,
  isPropertyName,
  overrideTokens,
  partEntries,
  pickerColor,
  pixels,
  resolveTokenValues,
  selectorMatches,
  withOverrides,
  selectorPart,
  tokenGroup,
  valueIn,
} from "./tokens";

const token = (
  category: string,
  name: string,
  value: string,
  modes: Record<string, string> = {},
  origin: ThemeTokenValue["origin"] = "base",
): ThemeTokenValue => ({ category, name, value, modes, origin });

describe("token groups", () => {
  it("puts every category into one of the editor's groups", () => {
    expect(TOKEN_GROUPS).toEqual(["color", "type", "spacing", "radius", "motion", "other"]);
    expect(tokenGroup("color")).toBe("color");
    for (const c of ["font", "font-size", "font-weight", "line-height"]) {
      expect(tokenGroup(c)).toBe("type");
    }
    expect(tokenGroup("space")).toBe("spacing");
    expect(tokenGroup("size")).toBe("spacing");
    expect(tokenGroup("radius")).toBe("radius");
    expect(tokenGroup("motion")).toBe("motion");
    // Everything else stays reachable under "More".
    expect(tokenGroup("shadow")).toBe("other");
    expect(tokenGroup("opacity")).toBe("other");
    expect(tokenGroup("something-new")).toBe("other");
  });
});

describe("token values", () => {
  it("writes pixels for unitless numbers of pixel categories only", () => {
    expect(cssValue("font-size", "14")).toBe("14px");
    expect(cssValue("radius", " 6.5 ")).toBe("6.5px");
    expect(cssValue("space", "1rem")).toBe("1rem");
    expect(cssValue("font-weight", "600")).toBe("600");
    expect(cssValue("line-height", "1.45")).toBe("1.45");
  });

  it("gives the colour picker #rrggbb values only", () => {
    expect(pickerColor("#3B5BDB")).toBe("#3b5bdb");
    expect(pickerColor("#abc")).toBe("#aabbcc");
    expect(pickerColor("$color.accent")).toBeNull();
    expect(pickerColor("rgb(0 0 0)")).toBeNull();
    expect(pickerColor("#abcd")).toBeNull();
  });

  it("reads pixel values", () => {
    expect(pixels("14px")).toBe(14);
    expect(pixels("13")).toBe(13);
    expect(pixels("calc(1px * 2)")).toBeNull();
    expect(pixels(undefined)).toBeNull();
  });

  it("knows mode values and draft keys", () => {
    const t = token("color", "bg", "#ffffff", { dark: "#000000" });
    expect(valueIn(t, null)).toBe("#ffffff");
    expect(valueIn(t, "dark")).toBe("#000000");
    expect(valueIn(t, "print")).toBe("#ffffff");
    expect(hasModeValue(t, "dark")).toBe(true);
    expect(hasModeValue(t, "print")).toBe(false);
    expect(hasModeValue(t, null)).toBe(true);
    expect(draftKey("color", "bg", null)).toBe("color.bg");
    expect(draftKey("color", "bg", "dark")).toBe("color.bg@dark");
  });
});

describe("resolving a theme for the preview", () => {
  const tokens = [
    token("color", "accent", "#2e5eaa", { dark: "#7aa7e8" }),
    token("color", "focus-ring", "$color.accent"),
    token("color", "unread", "var(--nail-color-accent)"),
    token("color", "soft", "color-mix(in srgb, $color.accent 12%, $color.bg)"),
    token("color", "bg", "#ffffff", { dark: "#1b1e24" }),
    token("font-size", "body", "14"),
    token("font-size", "small", "calc($font-size.body * 12.5 / 14)"),
    token("color", "loop", "$color.loop"),
    token("color", "unknown", "$color.nowhere"),
    token("color", "fallback", "var(--nail-color-gone, #123456)"),
  ];

  it("follows KDL and CSS references through the same theme", () => {
    const v = resolveTokenValues(tokens, null);
    expect(v.get("color.focus-ring")).toBe("#2e5eaa");
    expect(v.get("color.unread")).toBe("#2e5eaa");
    expect(v.get("color.soft")).toBe("color-mix(in srgb, #2e5eaa 12%, #ffffff)");
    expect(v.get("font-size.small")).toBe("calc(14px * 12.5 / 14)");
    // A token naming itself or nothing stays as written; a var() fallback is used.
    expect(v.get("color.loop")).toBe("$color.loop");
    expect(v.get("color.unknown")).toBe("$color.nowhere");
    expect(v.get("color.fallback")).toBe("#123456");
  });

  it("uses the mode's values, and drafts before everything", () => {
    const dark = resolveTokenValues(tokens, "dark");
    expect(dark.get("color.focus-ring")).toBe("#7aa7e8");
    expect(dark.get("color.soft")).toBe("color-mix(in srgb, #7aa7e8 12%, #1b1e24)");
    const drafted = resolveTokenValues(
      tokens,
      null,
      new Map([
        ["color.accent", "#ff0066"],
        ["font-size.body", "16"],
      ]),
    );
    expect(drafted.get("color.unread")).toBe("#ff0066");
    expect(drafted.get("font-size.small")).toBe("calc(16px * 12.5 / 14)");
    const darkDraft = resolveTokenValues(
      tokens,
      "dark",
      new Map([["color.accent@dark", "#00ff00"]]),
    );
    expect(darkDraft.get("color.focus-ring")).toBe("#00ff00");
  });

  it("styles the preview from the resolved values, the app's tokens for missing ones", () => {
    const styles = previewStyles(resolveTokenValues(tokens, null));
    expect(styles.primary?.["background-color"]).toBe("#2e5eaa");
    expect(styles.list?.["background-color"]).toBe("#ffffff");
    expect(styles.snippet?.["font-size"]).toBe("calc(14px * 12.5 / 14)");
    expect(styles.frame?.["background-color"]).toBe("var(--nail-color-canvas)");
  });

  // Review finding (ADR 0138): the preview shows the Appearance overrides,
  // which win over the theme in every mode.
  it("lays the Appearance overrides over the drafts for the preview", () => {
    const overrides = overrideTokens({ accent: "#aa3366", font_size: 16 });
    expect([...overrides]).toEqual([
      ["color.accent", "#aa3366"],
      ["font-size.body", "16"],
    ]);
    const drafts = new Map([["color.accent", "#2255ff"]]);
    const base = resolveTokenValues(tokens, null, withOverrides(drafts, overrides, null));
    expect(base.get("color.accent")).toBe("#aa3366");
    expect(previewStyles(base).primary?.["background-color"]).toBe("#aa3366");
    const dark = resolveTokenValues(tokens, "dark", withOverrides(drafts, overrides, "dark"));
    expect(dark.get("color.accent")).toBe("#aa3366");
    expect(overrideTokens({})).toEqual(new Map());
  });
});

describe("parts", () => {
  const rule = (selector: string, origin: ThemePartRule["origin"]): ThemePartRule => ({
    selector,
    properties: { color: "red" },
    origin,
  });

  it("groups rules by selector and adds parts without a rule", () => {
    const entries = partEntries(
      [rule("toolbar", "base"), rule("app", "base"), rule("toolbar", "own")],
      ["message-list.row", "app", ""],
    );
    expect(entries.map((e) => e.selector)).toEqual(["app", "message-list.row", "toolbar"]);
    expect(entries.find((e) => e.selector === "toolbar")).toMatchObject({ own: true });
    expect(entries.find((e) => e.selector === "toolbar")?.rules).toHaveLength(2);
    expect(entries.find((e) => e.selector === "message-list.row")?.rules).toEqual([]);
  });

  it("tells which part a selector targets", () => {
    expect(selectorPart('split[orientation="horizontal"] split.divider:hover')).toBe(
      "split.divider",
    );
    expect(selectorMatches("message-list.row:hover", "message-list.row")).toBe(true);
    expect(selectorMatches("message-list.row.subject", "message-list.row")).toBe(false);
    expect(selectorMatches("message-list.row.subject", "message-list.row", true)).toBe(true);
  });

  it("checks selectors and property names", () => {
    expect(isPartSelector("message-list.row")).toBe(true);
    expect(isPartSelector('message-list.row[state~="unread"]:hover')).toBe(true);
    expect(isPartSelector('split[orientation="horizontal"] split.divider')).toBe(true);
    expect(isPartSelector("Message-List")).toBe(false);
    expect(isPartSelector("row { color: red }")).toBe(false);
    expect(isPartSelector("")).toBe(false);
    expect(isPropertyName("background-color")).toBe(true);
    expect(isPropertyName("--custom")).toBe(false);
    expect(isPropertyName("color;")).toBe(false);
  });
});
