/**
 * Event cards stay readable in every built-in theme (WP164): the card's
 * text is the theme's `fg` on the theme's `bg` tinted with `EVENT_TINT` per
 * cent of the calendar colour, so the contrast is checked for the palette,
 * the demo calendars' colours and the extremes (black, white, pure
 * primaries) against Frost, Sage, Graphite, Nocturne and the base theme's
 * light and dark modes — WCAG AA (4.5:1) for the small card text. High
 * contrast shows no tint (`skin.ts`), so there `fg` on `bg` must reach AAA.
 * Also: which colour values reach the stylesheet at all.
 */
import { defaultTheme } from "../../api/mockData";
import { BUILTIN_THEME_FILES } from "../appearance/themes";
import {
  CALENDAR_COLORS,
  EVENT_TINT,
  contrastRatio,
  eventCardDeclarations,
  isCssColor,
  mixHex,
  paletteColorOf,
  parseHex,
  swatchDeclarations,
} from "./colors";

/** Colours a calendar can have: the palette, the demo data and extremes. */
const COLORS = [
  ...CALENDAR_COLORS.map((c) => c.value),
  "#3b82f6",
  "#ef4444",
  "#16a34a",
  "#a855f7",
  "#000000",
  "#ffffff",
  "#ffff00",
  "#00ffff",
  "#ff00ff",
  "#ff0000",
  "#0000ff",
];

function rgb(hex: string): [number, number, number] {
  const value = parseHex(hex);
  if (!value) throw new Error(`not a hex colour: ${hex}`);
  return value;
}

/** bg / fg of each light and dark theme the cards are tinted in. */
function tintedThemes(): [string, string, string][] {
  const base = defaultTheme();
  const out: [string, string, string][] = [
    ["default light", base.tokens.color?.bg ?? "", base.tokens.color?.fg ?? ""],
    ["default dark", base.modes.dark?.color?.bg ?? "", base.modes.dark?.color?.fg ?? ""],
  ];
  for (const [id, file] of BUILTIN_THEME_FILES) {
    if (file.mode === "high-contrast") continue;
    out.push([id, file.tokens.color?.bg ?? "", file.tokens.color?.fg ?? ""]);
  }
  return out;
}

describe("event card colours", () => {
  it("covers the four tinted built-in themes and High contrast", () => {
    expect([...BUILTIN_THEME_FILES.keys()]).toEqual([
      "frost",
      "graphite",
      "high-contrast",
      "nocturne",
      "sage",
    ]);
    expect(tintedThemes()).toHaveLength(6);
  });

  it.each(tintedThemes())("%s: card text reaches 4.5:1 on every calendar colour", (_n, bg, fg) => {
    for (const color of COLORS) {
      const card = mixHex(color, EVENT_TINT, bg);
      expect(contrastRatio(rgb(fg), card), `${color} on ${bg}`).toBeGreaterThanOrEqual(4.5);
    }
  });

  it("High contrast: white on black without a tint reaches 7:1", () => {
    const hc = BUILTIN_THEME_FILES.get("high-contrast");
    const bg = hc?.tokens.color?.bg ?? "";
    const fg = hc?.tokens.color?.fg ?? "";
    expect(contrastRatio(rgb(fg), rgb(bg))).toBeGreaterThanOrEqual(7);
  });

  it("mixes like CSS color-mix in sRGB", () => {
    expect(mixHex("#ffffff", 50, "#000000")).toEqual([127.5, 127.5, 127.5]);
    expect(mixHex("#ff0000", 100, "#000000")).toEqual([255, 0, 0]);
    expect(contrastRatio([0, 0, 0], [255, 255, 255])).toBeCloseTo(21, 5);
    expect(parseHex("#abc")).toEqual([170, 187, 204]);
    expect(parseHex("#11223380")).toEqual([17, 34, 51]);
    expect(parseHex("blue")).toBeNull();
    expect(() => mixHex("blue", 10, "#000000")).toThrow(/not a hex colour/);
  });
});

describe("colour values from data", () => {
  it("accepts hex colours and keywords only", () => {
    for (const ok of ["#abc", "#AABBCC", "#aabbcc80", "rebeccapurple", " #3b82f6 "]) {
      expect(isCssColor(ok), ok).toBe(true);
    }
    for (const bad of [
      null,
      undefined,
      "",
      "#abcd1",
      "red; background: url(x)",
      "var(--x)",
      "rgb(1,2,3)",
      "expression(alert(1))",
    ]) {
      expect(isCssColor(bad), String(bad)).toBe(false);
    }
  });

  it("builds the card and swatch declarations", () => {
    expect(eventCardDeclarations("#2563eb")).toEqual({
      "border-left-color": "#2563eb",
      "background-color": `color-mix(in srgb, #2563eb ${String(EVENT_TINT)}%, var(--nail-color-bg))`,
    });
    expect(eventCardDeclarations(null)).toEqual({});
    expect(eventCardDeclarations("url(x)")).toEqual({});
    expect(swatchDeclarations("#16a34a")).toEqual({
      "background-color": "#16a34a",
      "border-color": "#16a34a",
    });
    expect(swatchDeclarations("}")).toEqual({});
  });

  it("finds palette colours case-insensitively and names each one", () => {
    expect(paletteColorOf("#9333EA")?.id).toBe("purple");
    expect(paletteColorOf("#ef4444")).toBeNull();
    expect(new Set(CALENDAR_COLORS.map((c) => c.id)).size).toBe(CALENDAR_COLORS.length);
    for (const c of CALENDAR_COLORS) expect(isCssColor(c.value)).toBe(true);
  });
});
