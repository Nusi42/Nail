/**
 * Text areas take the UI face, monospace only for code (R8 check of the
 * M5a follow-up "Monospace text areas"): the base stylesheet makes every
 * text area inherit the UI font, and among the composer's, the rules' and
 * the merge dialog's text areas only those that hold code (HTML source,
 * rules.kdl text, KDL to merge) set `$font.mono` in defaults/theme.kdl.
 */
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { BASE_CSS } from "./compile";

const read = (file: string) => readFileSync(resolve(process.cwd(), file), "utf8");

const theme = read("../defaults/theme.kdl");

/** The sources whose text areas are checked. */
const SOURCES = [
  "src/features/composer/Composer.tsx",
  "src/features/composer/HtmlEditor.tsx",
  "src/features/rules/TextView.tsx",
  "src/features/customization/MergeDialog.tsx",
];

/** The text areas that hold code. */
const CODE_TEXT_AREAS = new Set(["composer.html.source", "rules.text.source", "merge-dialog.text"]);

/** The parts of the `<textarea>` elements in `source`. */
function textAreaParts(source: string): string[] {
  return [...source.matchAll(/<textarea\b[^>]*?data-part="([^"]+)"/gs)].map((m) => m[1] ?? "");
}

/** The declarations of `part "<name>"` in defaults/theme.kdl (one line or a block). */
function themeRule(name: string): string {
  const start = theme.indexOf(`part "${name}" {`);
  if (start < 0) return "";
  return theme.slice(start, theme.indexOf("}", start));
}

describe("text areas (R8 check)", () => {
  it("inherit the UI face from the base stylesheet", () => {
    expect(BASE_CSS).toContain("button, input, select, textarea { font: inherit; }");
  });

  it("are monospace only where they hold code", () => {
    const parts = SOURCES.flatMap((file) => textAreaParts(read(file)));
    expect(parts).toEqual(expect.arrayContaining([...CODE_TEXT_AREAS, "composer.body"]));
    for (const part of parts) {
      expect(themeRule(part).includes("$font.mono"), part).toBe(CODE_TEXT_AREAS.has(part));
    }
  });
});
