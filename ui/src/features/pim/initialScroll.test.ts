/**
 * P1 WP192: the time grid's opening scroll position follows the scroll
 * area's size until the user scrolls it.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { installFakeResizeObserver, resized } from "../../test/resizeObserver";
import { keepInitialScroll } from "./initialScroll";

beforeEach(() => {
  installFakeResizeObserver();
});
afterEach(() => {
  vi.unstubAllGlobals();
});

describe("keepInitialScroll", () => {
  it("sets the position now and again when the area changes size", () => {
    const el = document.createElement("div");
    let wanted = 288;
    const stop = keepInitialScroll(el, () => wanted);
    expect(el.scrollTop).toBe(288);
    // The layout settles: a taller area allows less scrolling.
    wanted = 240;
    resized(el);
    expect(el.scrollTop).toBe(240);
    stop();
    wanted = 96;
    resized(el);
    expect(el.scrollTop).toBe(240);
  });

  it("leaves the position alone once the user moved the grid", () => {
    for (const type of ["wheel", "pointerdown", "keydown", "touchstart"]) {
      const el = document.createElement("div");
      let wanted = 384;
      const stop = keepInitialScroll(el, () => wanted);
      el.dispatchEvent(new Event(type));
      el.scrollTop = 1000;
      wanted = 48;
      resized(el);
      expect(el.scrollTop, type).toBe(1000);
      stop();
    }
  });

  it("works without ResizeObserver (sets the position once)", () => {
    vi.stubGlobal("ResizeObserver", undefined);
    const el = document.createElement("div");
    const stop = keepInitialScroll(el, () => 144);
    expect(el.scrollTop).toBe(144);
    stop();
  });
});
