/**
 * M5d WP183: the Appearance / theme editor skin is written on the token
 * schema only, closes every block, and styles the parts the components
 * emit.
 */
import { readFileSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { defaultTheme } from "../../api/mockData";
import {
  APPEARANCE_SKIN_CSS,
  THEME_EDITOR_CONTAINER,
  THEME_EDITOR_MEDIUM,
  THEME_EDITOR_NARROW,
  THEME_EDITOR_STACKED,
  THEME_PANEL_CONTAINER,
  THEME_PANEL_STACKED,
  THEME_PREVIEW_CONTAINER,
} from "./skin";

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

  describe("P1 (WP192)", () => {
    /** The declarations of the first rule whose selector is exactly `selector`. */
    const rule = (selector: string) => {
      const start = APPEARANCE_SKIN_CSS.indexOf(`${selector} {`);
      expect(start, selector).toBeGreaterThanOrEqual(0);
      return APPEARANCE_SKIN_CSS.slice(start, APPEARANCE_SKIN_CSS.indexOf("}", start));
    };

    it("lines the token rows up: name, origin tag, then the controls at a fixed share", () => {
      const row = rule('[data-part="theme-editor.token"]');
      expect(row).toMatch(/display: grid;/);
      // The controls column is a share of the row (not `auto`), so every
      // row's tag ends at the same edge whatever the value's length.
      expect(row).toMatch(/grid-template-columns: minmax\(0, 1fr\) auto minmax\(0, \d+%\);/);
      expect(rule('[data-part="theme-editor.token.text"]')).toMatch(/display: contents;/);
      const tag = rule(
        '[data-part="theme-editor.token.text"] > [data-part="tag"][data-kind="origin"]',
      );
      expect(tag).toMatch(/grid-column: 2;/);
      expect(tag).toMatch(/white-space: nowrap;/);
      expect(rule('[data-part="theme-editor.token.controls"]')).toMatch(/grid-column: 3;/);
    });

    it("stacks a token's controls under its name in a narrow editor", () => {
      expect(APPEARANCE_SKIN_CSS).toMatch(
        new RegExp(
          `@container ${THEME_EDITOR_CONTAINER} \\(max-width: ${THEME_EDITOR_STACKED}\\) \\{[^@]*theme-editor\\.token.controls"\\] \\{ grid-column: 1 / -1;`,
        ),
      );
      expect(APPEARANCE_SKIN_CSS).toContain(`container: ${THEME_PREVIEW_CONTAINER} / inline-size;`);
      // Beside the preview the panel itself is the measure (P1 review: long values were cut off).
      expect(APPEARANCE_SKIN_CSS).toContain(
        `\n[data-part="theme-editor.panel"] { container: ${THEME_PANEL_CONTAINER} / inline-size;`,
      );
      expect(APPEARANCE_SKIN_CSS).toMatch(
        new RegExp(
          `@container ${THEME_PANEL_CONTAINER} \\(max-width: ${THEME_PANEL_STACKED}\\) \\{[^@]*theme-editor\\.token.controls"\\] \\{ grid-column: 1 / -1;`,
        ),
      );
    });

    it("keeps the light and dark theme fields on one line without a separator", () => {
      expect(
        rule('[data-part="appearance.pair"] > [data-part="form.field"] + [data-part="form.field"]'),
      ).toMatch(/border-top: 0;/);
      // P1 review: the labels do not wrap beside their selects at 1024 px.
      expect(
        rule('[data-part="appearance.pair"] > [data-part="form.field"] > [data-part="form.label"]'),
      ).toMatch(/white-space: nowrap;/);
    });

    it("keeps the live preview in sight at every width (P1 review)", () => {
      /** The body of the `@container` block for `width`. */
      const block = (width: string) => {
        const head = `@container ${THEME_EDITOR_CONTAINER} (max-width: ${width}) {`;
        const start = APPEARANCE_SKIN_CSS.indexOf(head);
        expect(start, width).toBeGreaterThanOrEqual(0);
        return APPEARANCE_SKIN_CSS.slice(start, APPEARANCE_SKIN_CSS.indexOf("\n}", start));
      };
      // Wide: tabs, panel, preview side by side, the preview at a real share.
      expect(rule('[data-part="theme-editor.body"]')).toMatch(
        /grid-template-columns: [^;]*minmax\(0, 3fr\) minmax\([^;]*, 2fr\);[^}]*grid-template-areas: "tabs panel preview";/,
      );
      // Medium: tabs above, panel and preview side by side.
      expect(block(THEME_EDITOR_MEDIUM)).toContain(
        `grid-template-areas: "tabs tabs" "panel preview";`,
      );
      // Narrow: one column with the preview above the panel, never after the token list.
      expect(block(THEME_EDITOR_NARROW)).toContain(
        `grid-template-areas: "tabs" "preview" "panel";`,
      );
      expect(parseFloat(THEME_EDITOR_NARROW)).toBeLessThan(parseFloat(THEME_EDITOR_MEDIUM));
      // The narrow block comes last, so it wins over the medium one.
      expect(APPEARANCE_SKIN_CSS.lastIndexOf(THEME_EDITOR_NARROW)).toBeGreaterThan(
        APPEARANCE_SKIN_CSS.lastIndexOf(THEME_EDITOR_MEDIUM),
      );
    });
  });
});
