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
import type { Calendar } from "../../api/generated";
import { defaultTheme } from "../../api/mockData";
import { BUILTIN_THEME_FILES } from "../appearance/themes";
import { contrastRatio, mixHex, parseHex } from "../../theme/contrast";
import {
  CALENDAR_COLORS,
  EVENT_TINT,
  MIN_COLOR_DISTANCE,
  colorDistance,
  displayColors,
  eventCardDeclarations,
  isCssColor,
  oklabOf,
  paletteColorOf,
  swatchDeclarations,
} from "./colors";
import { colorOf } from "./common";
import calendarColorVectors from "../../../../crates/nail-api/fixtures/ts-vectors/calendar-colors.json";

/**
 * The colours the demo seeds give calendars (R8): the mock CalDAV server's
 * `personal` (`nail_testkit_net::SEED_PERSONAL_COLOR`) and the demo-multi
 * holiday subscription (`nail-core` `MULTI_HOLIDAYS_COLOR`), from the
 * vectors the Rust test `calendar_color_vectors` writes.
 */
const SEED_COLORS = calendarColorVectors.cases
  .filter((c) => c.fn === "seedCalendarColors")
  .flatMap((c) => c.out)
  // `#rrggbbaa` (the CalDAV form, opaque) as `#rrggbb`: the contrast checks read six digits.
  .map((c) => c.slice(0, 7));

/** Colours a calendar can have: the palette, the demo data and extremes. */
const COLORS = [
  ...CALENDAR_COLORS.map((c) => c.value),
  ...SEED_COLORS,
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
  it("covers the five tinted built-in themes and High contrast", () => {
    expect([...BUILTIN_THEME_FILES.keys()]).toEqual([
      "frost",
      "graphite",
      "high-contrast",
      "nocturne",
      "paper",
      "sage",
    ]);
    expect(tintedThemes()).toHaveLength(7);
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
    // Like nail_util::color: no alpha.
    expect(parseHex("#11223380")).toBeNull();
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

/** A calendar for the display rule: only id, colour and visibility matter. */
function cal(id: string, color: string | null, visible = true): Calendar {
  return {
    id,
    source_id: "src",
    account_id: null,
    settings: {
      name: id,
      description: null,
      color,
      visible,
      reminders: true,
      default_alarms: [],
      order: 0,
    },
    components: ["event"],
    read_only: false,
    timezone: null,
  };
}

const BLUE = CALENDAR_COLORS[0]?.value ?? "";

describe("calendar colours that stay apart (R8, ADR 0251)", () => {
  it("measures the OKLab distance", () => {
    expect(colorDistance("#ffffff", "#ffffff")).toBe(0);
    // White and black: L from 1 to 0.
    expect(colorDistance("#ffffff", "#000000")).toBeCloseTo(1, 3);
    expect(colorDistance("#fff", "#ffffffff")).toBeCloseTo(0, 6);
    expect(colorDistance("blue", "#0000ff")).toBeNull();
    expect(oklabOf("#2563eb80")).toEqual(oklabOf("#2563eb"));
    // The old demo "Personal" blue is too close to the palette blue; the R7 red to the palette red.
    expect(colorDistance("#3366cc", BLUE)).toBeLessThan(MIN_COLOR_DISTANCE);
    expect(colorDistance("#dd3333", "#dc2626")).toBeLessThan(MIN_COLOR_DISTANCE);
  });

  it("never counts two palette colours as too close", () => {
    for (const a of CALENDAR_COLORS) {
      for (const b of CALENDAR_COLORS) {
        if (a.id === b.id) continue;
        expect(colorDistance(a.value, b.value), `${a.id} / ${b.id}`).toBeGreaterThanOrEqual(
          MIN_COLOR_DISTANCE,
        );
      }
    }
  });

  it("shows a server colour too close to an earlier calendar's in the next distinct palette colour", () => {
    const shown = displayColors([cal("palette", BLUE), cal("server", "#3366ccff")]);
    expect(shown.get("palette")).toBe(BLUE);
    const moved = shown.get("server") ?? "";
    expect(moved).not.toBe("#3366ccff");
    expect(paletteColorOf(moved)).not.toBeNull();
    expect(colorDistance(moved, BLUE)).toBeGreaterThanOrEqual(MIN_COLOR_DISTANCE);
    expect(colorDistance(moved, "#3366cc")).toBeGreaterThanOrEqual(MIN_COLOR_DISTANCE);
  });

  it("keeps palette colours as picked, even when the server colour came first", () => {
    const shown = displayColors([cal("server", "#3366cc"), cal("picked", BLUE)]);
    expect(shown.get("picked")).toBe(BLUE);
    expect(shown.get("server")).not.toBe("#3366cc");
    // Two calendars picked in the same palette colour stay as picked.
    const same = displayColors([cal("a", BLUE), cal("b", BLUE.toUpperCase())]);
    expect([same.get("a"), same.get("b")]).toEqual([BLUE, BLUE.toUpperCase()]);
  });

  it("moves only the later of two close server colours, and leaves distinct ones alone", () => {
    const shown = displayColors([
      cal("first", "#3a6fd8"),
      cal("second", "#3366cc"),
      cal("apart", "#a16207"),
    ]);
    expect(shown.get("first")).toBe("#3a6fd8");
    expect(shown.get("apart")).toBe("#a16207");
    const second = shown.get("second") ?? "";
    expect(second).not.toBe("#3366cc");
    expect(colorDistance(second, "#3a6fd8")).toBeGreaterThanOrEqual(MIN_COLOR_DISTANCE);
    expect(colorDistance(second, "#a16207")).toBeGreaterThanOrEqual(MIN_COLOR_DISTANCE);
  });

  it("ignores hidden calendars, missing colours and keywords", () => {
    const shown = displayColors([
      cal("hidden", BLUE, false),
      cal("server", "#3366cc"),
      cal("none", null),
      cal("keyword", "rebeccapurple"),
    ]);
    expect(shown.get("server")).toBe("#3366cc");
    expect(shown.get("none")).toBeNull();
    expect(shown.get("keyword")).toBe("rebeccapurple");
  });

  it("gives the demo seeds colours distinct from the palette and from each other", () => {
    expect(SEED_COLORS).toHaveLength(2);
    const all = [...SEED_COLORS, ...CALENDAR_COLORS.map((c) => c.value)];
    for (const seed of SEED_COLORS) {
      expect(oklabOf(seed), seed).not.toBeNull();
      for (const other of all) {
        if (other === seed) continue;
        expect(colorDistance(seed, other), `${seed} / ${other}`).toBeGreaterThanOrEqual(
          MIN_COLOR_DISTANCE,
        );
      }
    }
  });

  it("is what an event without its own colour is shown in", () => {
    const calendars = [cal("palette", BLUE), cal("server", "#3366cc")];
    expect(colorOf(calendars, "server", null)).toBe(displayColors(calendars).get("server"));
    expect(colorOf(calendars, "server", "#123456")).toBe("#123456");
    expect(colorOf(undefined, "server", null)).toBeNull();
  });
});
