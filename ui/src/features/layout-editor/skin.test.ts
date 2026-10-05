/**
 * The layout editor's skin (M5d WP181): token-only and well-formed; the
 * parts it styles are checked against the rendered editor in
 * `editor.test.tsx`.
 */
import { must } from "../../test/must";
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
  // P1 (WP192): at 1024 px the palette leaves the window less room than its
  // panes need; the canvas scrolls sideways instead of cutting the last pane.
  it("lets the canvas scroll to panes that do not fit beside the palette", () => {
    const canvas = must(
      /\[data-part="layout-editor\.canvas"\] \{[^}]*\}/.exec(LAYOUT_EDITOR_SKIN_CSS),
    )[0];
    expect(canvas).toMatch(/overflow: auto;/);
    expect(LAYOUT_EDITOR_SKIN_CSS).toContain(
      '[data-part="layout-editor.canvas"] > * { min-width: min-content; min-height: 0; }',
    );
    // The hint takes a wide line; the buttons wrap below it when needed.
    expect(LAYOUT_EDITOR_SKIN_CSS).toContain('[data-part="layout-editor.hint"] { flex: 1 1 28em;');
  });
});
