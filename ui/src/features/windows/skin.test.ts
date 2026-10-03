/**
 * The window manager's skin (M5c WP171): token-only, well-formed, and it
 * styles every part the window layer emits.
 */
import { WINDOWS_SKIN_CSS, WINDOW_PARTS } from "./skin";

describe("window manager skin", () => {
  it("styles the frame, title bar, handles, snap preview and scrim", () => {
    for (const part of [
      "window-layer",
      "window-scrim",
      "window",
      "window.header",
      "window.title",
      "window.hint",
      "window.status",
      "window.maximize",
      "window.maximize.icon",
      "window.handle",
      "window-snap-preview",
    ]) {
      expect(WINDOW_PARTS).toContain(part);
    }
    for (const edge of ["n", "e", "s", "w", "ne", "se", "sw", "nw"]) {
      expect(WINDOWS_SKIN_CSS).toContain(`[data-part="window.handle"][data-edge="${edge}"]`);
    }
    for (const snap of ["left", "right"]) {
      expect(WINDOWS_SKIN_CSS).toContain(`[data-part="window-snap-preview"][data-snap="${snap}"]`);
    }
  });

  it("closes every block it opens", () => {
    let depth = 0;
    for (const char of WINDOWS_SKIN_CSS) {
      if (char === "{") depth += 1;
      if (char === "}") depth -= 1;
      expect(depth).toBeGreaterThanOrEqual(0);
    }
    expect(depth).toBe(0);
  });

  it("takes colours only from tokens", () => {
    expect(WINDOWS_SKIN_CSS).not.toMatch(/#[0-9a-f]{3,8}\b/i);
    expect(WINDOWS_SKIN_CSS).not.toMatch(/\brgba?\(|\bhsla?\(/i);
    expect(WINDOWS_SKIN_CSS).not.toMatch(/url\(|@import|!important/);
    for (const color of WINDOWS_SKIN_CSS.matchAll(
      /(?:^|[\s;{])(?:color|background-color|border-color):\s*([^;]+);/g,
    )) {
      expect(color[1]).toMatch(/^(var\(--nail-|color-mix\(|transparent)/);
    }
  });

  it("lets the pointer through the layer to the main window, but not through windows or the scrim", () => {
    expect(WINDOWS_SKIN_CSS).toMatch(/\[data-part="window-layer"\] \{[^}]*pointer-events: none/);
    expect(WINDOWS_SKIN_CSS).toMatch(
      /\[data-part="window-layer"\] > \[data-part="window"\] \{[^}]*pointer-events: auto/,
    );
    expect(WINDOWS_SKIN_CSS).toMatch(/\[data-part="window-scrim"\] \{[^}]*pointer-events: auto/);
  });

  it("shows windows that are not the active one (the base rules hid them while windows were modal)", () => {
    // Same specificity as the old base rule, emitted later: the skin wins.
    expect(WINDOWS_SKIN_CSS).toContain(
      '[data-part="window-layer"] > [data-part="window"]:not([data-state~="active"]) { display: flex; }',
    );
  });

  it("never sets where a window is or how big it is (geometry rules do)", () => {
    const frame = /\[data-part="window-layer"\] > \[data-part="window"\] \{([^}]*)\}/.exec(
      WINDOWS_SKIN_CSS,
    )?.[1];
    expect(frame).toBeDefined();
    expect(frame).not.toMatch(/(^|[\s;])(left|top|width|height):/);
  });
});
