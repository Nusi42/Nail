/**
 * M5d WP183: the Appearance / theme editor skin is written on the token
 * schema only, closes every block, and styles the parts the components
 * emit.
 */
import { readFileSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { defaultTheme } from "../../api/mockData";
import { APPEARANCE_SKIN_CSS } from "./skin";

const dir = resolve(process.cwd(), "src/features/appearance");

/** Every `data-part` literal of this feature's components. */
function emittedParts(): Set<string> {
  const parts = new Set<string>();
  for (const file of readdirSync(dir)) {
    if (!file.endsWith(".tsx") || file.includes(".test.")) continue;
    const text = readFileSync(join(dir, file), "utf8");
    for (const m of text.matchAll(/data-part="([a-z0-9.-]+)"/g)) parts.add(m[1] ?? "");
  }
  return parts;
}

describe("appearance skin", () => {
  it("takes colours and sizes only from tokens", () => {
    expect(APPEARANCE_SKIN_CSS.match(/#[0-9a-f]{3,8}\b/gi) ?? []).toEqual([]);
    const theme = defaultTheme();
    const known = new Set(
      Object.entries(theme.tokens).flatMap(([category, names]) =>
        Object.keys(names).map((name) => `--nail-${category}-${name}`),
      ),
    );
    // Every token it names exists in the base theme, or carries a fallback.
    for (const m of APPEARANCE_SKIN_CSS.matchAll(/var\((--nail-[a-z0-9-]+)(,)?/g)) {
      const name = m[1] ?? "";
      if (m[2] === undefined) expect(known, name).toContain(name);
    }
  });

  it("closes each block it opens", () => {
    const opened = APPEARANCE_SKIN_CSS.split("{").length;
    const closed = APPEARANCE_SKIN_CSS.split("}").length;
    expect(opened).toBe(closed);
  });

  it("selects parts only, and styles the editor's own parts", () => {
    expect(APPEARANCE_SKIN_CSS).not.toMatch(/(^|\s)\.[a-z]/m);
    const emitted = emittedParts();
    for (const part of [
      "theme-editor",
      "theme-editor.body",
      "theme-editor.token",
      "theme-editor.part-button",
      "theme-editor.preview",
      "theme-dialog",
      "appearance.history",
      "appearance.slider",
    ]) {
      expect(emitted, part).toContain(part);
      expect(APPEARANCE_SKIN_CSS, part).toContain(`[data-part="${part}"]`);
    }
  });
});
