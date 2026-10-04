/**
 * M5d review (ADR 0138): the theme editor's contrast check and the
 * history buttons' Ctrl+Z / Ctrl+Y keys.
 */
import { describe, expect, it } from "vitest";
import { contrastProblems, contrastRatio, parseHex } from "./contrast";
import { historyKey } from "../../state/editHistory";

describe("contrast", () => {
  it("parses hex colours only", () => {
    expect(parseHex("#fff")).toEqual([255, 255, 255]);
    expect(parseHex(" #1A2b3C ")).toEqual([26, 43, 60]);
    expect(parseHex("red")).toBeNull();
    expect(parseHex("color-mix(in srgb, #fff 50%, #000)")).toBeNull();
  });

  it("computes WCAG contrast ratios", () => {
    expect(contrastRatio("#000000", "#ffffff")).toBeCloseTo(21, 5);
    expect(contrastRatio("#ffffff", "#ffffff")).toBeCloseTo(1, 5);
    expect(contrastRatio("#767676", "#ffffff")).toBeCloseTo(4.54, 2);
    expect(contrastRatio("var(--x)", "#ffffff")).toBeNull();
  });

  it("reports unreadable text pairs, worst first, and skips unknown values", () => {
    const values = new Map([
      ["color.fg", "#ffffff"],
      ["color.fg-muted", "#cccccc"],
      ["color.bg", "#ffffff"],
      ["color.bg-subtle", "#f5f5f5"],
      ["color.bg-raised", "var(--nail-color-bg)"],
      ["color.accent", "#2e5eaa"],
      ["color.accent-fg", "#ffffff"],
    ]);
    const problems = contrastProblems(values);
    expect(problems.map((p) => `${p.fg}/${p.bg}`)).toEqual([
      "color.fg/color.bg",
      "color.fg/color.bg-subtle",
      "color.fg-muted/color.bg",
    ]);
    expect(problems[0]?.ratio).toBeCloseTo(1, 5);
    expect(problems[0]?.needed).toBe(4.5);
    expect(
      contrastProblems(
        new Map([
          ["color.fg", "#1a1a1a"],
          ["color.bg", "#ffffff"],
        ]),
      ),
    ).toEqual([]);
  });
});

describe("history keys", () => {
  const key = (
    k: string,
    mods: Partial<Record<"ctrlKey" | "metaKey" | "shiftKey" | "altKey", boolean>> = {},
  ) =>
    historyKey({ key: k, ctrlKey: false, metaKey: false, shiftKey: false, altKey: false, ...mods });

  it("maps Ctrl+Z to undo, Ctrl+Y and Ctrl+Shift+Z to redo", () => {
    expect(key("z", { ctrlKey: true })).toBe("undo");
    expect(key("Z", { ctrlKey: true, shiftKey: true })).toBe("redo");
    expect(key("y", { ctrlKey: true })).toBe("redo");
    expect(key("z", { metaKey: true })).toBe("undo");
    expect(key("z")).toBeNull();
    expect(key("z", { ctrlKey: true, altKey: true })).toBeNull();
    expect(key("y", { ctrlKey: true, shiftKey: true })).toBeNull();
    expect(key("x", { ctrlKey: true })).toBeNull();
  });
});
