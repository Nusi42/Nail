/**
 * The layout editor's skin (M5d WP181): token-only and well-formed; the
 * parts it styles are checked against the rendered editor in
 * `editor.test.tsx`.
 */
import { LAYOUT_EDITOR_SKIN_CSS } from "./skin";

describe("layout editor skin", () => {
  it("closes every block it opens", () => {
    let depth = 0;
    for (const char of LAYOUT_EDITOR_SKIN_CSS) {
      if (char === "{") depth += 1;
      if (char === "}") depth -= 1;
      expect(depth).toBeGreaterThanOrEqual(0);
    }
    expect(depth).toBe(0);
  });

  it("uses theme tokens only: no literal colours", () => {
    expect(LAYOUT_EDITOR_SKIN_CSS).not.toMatch(/#[0-9a-f]{3,8}\b/i);
    expect(LAYOUT_EDITOR_SKIN_CSS).not.toMatch(/\brgba?\(|\bhsla?\(/);
    for (const [, name] of LAYOUT_EDITOR_SKIN_CSS.matchAll(/var\((--[a-z0-9-]+)/g)) {
      expect(name).toMatch(/^--nail-/);
    }
  });

  it("styles every drop zone and the dragging state", () => {
    for (const zone of ["left", "right", "top", "bottom", "swap", "into"]) {
      expect(LAYOUT_EDITOR_SKIN_CSS).toContain(
        `[data-part="layout-editor.target"][data-zone="${zone}"]`,
      );
    }
    expect(LAYOUT_EDITOR_SKIN_CSS).toContain('[data-part="layout-editor.frame"][data-dragging]');
    expect(LAYOUT_EDITOR_SKIN_CSS).toContain(
      '[data-part="layout-editor.pane"][data-state~="hidden"]',
    );
  });
});
