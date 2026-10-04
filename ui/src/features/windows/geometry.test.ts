/**
 * The window manager's geometry (M5c WP171): placing, moving, resizing,
 * snapping, maximizing and keeping windows on the page.
 */
import type { WindowGeometry } from "../../api/generated";
import {
  MAX_WINDOW_EDGE,
  WINDOW_CASCADE_OFFSET,
  WINDOW_KEYBOARD_STEP,
  WINDOW_MIN_VISIBLE,
  isValidGeometry,
} from "../../state/windows";
import {
  RESIZE_EDGES,
  SNAP_EDGE,
  cascadeSlots,
  centredOn,
  defaultRect,
  fitSize,
  initialGeometry,
  insidePage,
  isArrowKey,
  fitsPage,
  keepOnPage,
  keyboardMove,
  keyboardResize,
  looseRect,
  maximizedRect,
  moveRect,
  normal,
  resizeRect,
  roundRect,
  sameGeometry,
  shownRect,
  snapRect,
  snapTarget,
} from "./geometry";

const page = { width: 1280, height: 800 };
const min = { width: 360, height: 240 };
const size = { width: 720, height: 520 };

describe("window geometry (M5c WP171)", () => {
  it("opens centred at the spec's size and cascades further windows of a kind", () => {
    expect(defaultRect(size, min, page)).toEqual({ x: 280, y: 140, width: 720, height: 520 });
    expect(defaultRect(size, min, page, 2)).toEqual({
      x: 280 + 2 * WINDOW_CASCADE_OFFSET,
      y: 140 + 2 * WINDOW_CASCADE_OFFSET,
      width: 720,
      height: 520,
    });
    // A page smaller than the window: the page wins, the window starts at 0 / 0.
    expect(defaultRect(size, min, { width: 300, height: 200 })).toEqual({
      x: 0,
      y: 0,
      width: 300,
      height: 200,
    });
    // Far cascades stay on the page.
    const far = defaultRect(size, min, page, 40);
    expect(far.x + far.width).toBeLessThanOrEqual(page.width);
    expect(far.y + far.height).toBeLessThanOrEqual(page.height);
  });

  it("restores remembered geometry, fitted to the page; a second window of a kind is cascaded and normal", () => {
    const remembered: WindowGeometry = {
      rect: { x: 100, y: 50, width: 600, height: 400 },
      maximized: true,
      snap: null,
    };
    expect(initialGeometry(remembered, size, min, page)).toEqual(remembered);
    const second = initialGeometry(remembered, size, min, page, 1);
    expect(second).toEqual({
      rect: { x: 124, y: 74, width: 600, height: 400 },
      maximized: false,
      snap: null,
    });
    // Remembered on a bigger page: moved back inside, size capped.
    const big = initialGeometry(
      { rect: { x: 1800, y: 900, width: 2000, height: 300 }, maximized: false, snap: "left" },
      size,
      min,
      page,
    );
    expect(big).toEqual({
      rect: { x: 0, y: 500, width: 1280, height: 300 },
      maximized: false,
      snap: "left",
    });
    // Smaller than the minimum (an older minimum): grown to it.
    expect(
      initialGeometry(
        { rect: { x: 10, y: 10, width: 50, height: 50 }, maximized: false, snap: null },
        size,
        min,
        page,
      ).rect,
    ).toEqual({ x: 10, y: 10, width: 360, height: 240 });
    expect(initialGeometry(null, size, min, page)).toEqual({
      rect: defaultRect(size, min, page),
      maximized: false,
      snap: null,
    });
  });

  it("keeps WINDOW_MIN_VISIBLE px on the page and the title bar below the top", () => {
    const rect = { x: 0, y: 0, width: 600, height: 400 };
    expect(keepOnPage({ ...rect, x: -2000, y: -50 }, page)).toEqual({
      ...rect,
      x: WINDOW_MIN_VISIBLE - 600,
      y: 0,
    });
    expect(keepOnPage({ ...rect, x: 5000, y: 5000 }, page)).toEqual({
      ...rect,
      x: page.width - WINDOW_MIN_VISIBLE,
      y: page.height - WINDOW_MIN_VISIBLE,
    });
    expect(insidePage({ ...rect, x: 1000, y: 700 }, page)).toEqual({ ...rect, x: 680, y: 400 });
  });

  it("shows windows maximized, snapped or fitted to a page that shrank", () => {
    const rect = { x: 900, y: 300, width: 700, height: 500 };
    expect(shownRect({ rect, maximized: true, snap: null }, min, page)).toEqual(
      maximizedRect(page),
    );
    expect(shownRect({ rect, maximized: false, snap: "left" }, min, page)).toEqual({
      x: 0,
      y: 0,
      width: 640,
      height: 800,
    });
    expect(shownRect({ rect, maximized: false, snap: "right" }, min, page)).toEqual({
      x: 640,
      y: 0,
      width: 640,
      height: 800,
    });
    // The page shrinks to 800 × 600: the window shrinks to the page and stays reachable,
    // while its remembered rectangle is untouched.
    const small = { width: 800, height: 600 };
    expect(shownRect(normal(rect), min, small)).toEqual({
      x: 800 - WINDOW_MIN_VISIBLE,
      y: 300,
      width: 700,
      height: 500,
    });
    expect(shownRect(normal(rect), min, { width: 500, height: 400 })).toEqual({
      x: 500 - WINDOW_MIN_VISIBLE,
      y: 300,
      width: 500,
      height: 400,
    });
    // A snapped half is never narrower than the minimum width (nor wider than the page).
    expect(snapRect("right", { width: 800, height: 240 }, page)).toEqual({
      x: 480,
      y: 0,
      width: 800,
      height: 800,
    });
    expect(snapRect("left", { width: 800, height: 240 }, { width: 600, height: 400 })).toEqual({
      x: 0,
      y: 0,
      width: 600,
      height: 400,
    });
  });

  it("keeps a window placed wholly on the page wholly on a page that shrank (P1)", () => {
    const rect = { x: 500, y: 250, width: 700, height: 500 };
    expect(fitsPage(rect, page)).toBe(true);
    expect(fitsPage({ ...rect, x: 900 }, page)).toBe(false);
    expect(fitsPage({ ...rect, x: -1 }, page)).toBe(false);
    const small = { width: 1000, height: 600 };
    expect(shownRect(normal(rect), min, small, true)).toEqual({
      x: 300,
      y: 100,
      width: 700,
      height: 500,
    });
    // Smaller than the window: it shrinks to the page and fills it.
    expect(shownRect(normal(rect), min, { width: 500, height: 400 }, true)).toEqual({
      x: 0,
      y: 0,
      width: 500,
      height: 400,
    });
    // The page grows again: the remembered place is back.
    expect(shownRect(normal(rect), min, page, true)).toEqual(rect);
  });

  it("moves within the page", () => {
    const start = { x: 100, y: 100, width: 400, height: 300 };
    expect(moveRect(start, 50.4, -20.6, page)).toEqual({ x: 150, y: 79, width: 400, height: 300 });
    expect(moveRect(start, -5000, -5000, page)).toEqual({
      x: WINDOW_MIN_VISIBLE - 400,
      y: 0,
      width: 400,
      height: 300,
    });
  });

  it("resizes from all eight handles, keeping the opposite edges and the minimum size", () => {
    const start = { x: 200, y: 200, width: 500, height: 400 };
    expect(RESIZE_EDGES).toHaveLength(8);
    expect(resizeRect(start, "e", 40, 99, min, page)).toEqual({ ...start, width: 540 });
    expect(resizeRect(start, "s", 99, 40, min, page)).toEqual({ ...start, height: 440 });
    expect(resizeRect(start, "w", 40, 0, min, page)).toEqual({ ...start, x: 240, width: 460 });
    expect(resizeRect(start, "n", 0, 40, min, page)).toEqual({ ...start, y: 240, height: 360 });
    expect(resizeRect(start, "se", 10, 20, min, page)).toEqual({
      ...start,
      width: 510,
      height: 420,
    });
    expect(resizeRect(start, "nw", -10, -20, min, page)).toEqual({
      x: 190,
      y: 180,
      width: 510,
      height: 420,
    });
    expect(resizeRect(start, "ne", 10, 10, min, page)).toEqual({
      x: 200,
      y: 210,
      width: 510,
      height: 390,
    });
    expect(resizeRect(start, "sw", 10, 10, min, page)).toEqual({
      x: 210,
      y: 200,
      width: 490,
      height: 410,
    });
    // Minimum size: the left / top edge stops where the minimum is reached.
    expect(resizeRect(start, "nw", 1000, 1000, min, page)).toEqual({
      x: 200 + 500 - 360,
      y: 200 + 400 - 240,
      width: 360,
      height: 240,
    });
    // Not past the page edges.
    expect(resizeRect(start, "se", 5000, 5000, min, page)).toEqual({
      ...start,
      width: page.width - 200,
      height: page.height - 200,
    });
    expect(resizeRect(start, "nw", -5000, -5000, min, page)).toEqual({
      x: 0,
      y: 0,
      width: 700,
      height: 600,
    });
  });

  it("snaps at the left and right page edges", () => {
    expect(snapTarget(0, page)).toBe("left");
    expect(snapTarget(SNAP_EDGE, page)).toBe("left");
    expect(snapTarget(SNAP_EDGE + 1, page)).toBeNull();
    expect(snapTarget(640, page)).toBeNull();
    expect(snapTarget(page.width - SNAP_EDGE, page)).toBe("right");
    expect(snapTarget(page.width + 10, page)).toBe("right");
  });

  it("lets a maximized window come loose under the pointer at its restored size", () => {
    const shown = maximizedRect(page);
    const restored = { x: 100, y: 100, width: 640, height: 400 };
    // Grabbed in the middle of the title bar, dragged 100 px down.
    expect(looseRect(shown, restored, { x: 640, y: 10 }, { x: 640, y: 110 }, min, page)).toEqual({
      x: 320,
      y: 100,
      width: 640,
      height: 400,
    });
    // Grabbed near the left: the pointer keeps its relative place.
    expect(looseRect(shown, restored, { x: 128, y: 10 }, { x: 128, y: 10 }, min, page).x).toBe(64);
  });

  it("moves and resizes in keyboard steps", () => {
    const start = { x: 100, y: 100, width: 400, height: 300 };
    const step = WINDOW_KEYBOARD_STEP;
    expect(keyboardMove(start, "ArrowRight", page)).toEqual({ ...start, x: 100 + step });
    expect(keyboardMove(start, "ArrowUp", page)).toEqual({ ...start, y: 100 - step });
    expect(keyboardResize(start, "ArrowRight", min, page)).toEqual({
      ...start,
      width: 400 + step,
    });
    expect(keyboardResize(start, "ArrowDown", min, page)).toEqual({
      ...start,
      height: 300 + step,
    });
    expect(keyboardResize(start, "ArrowLeft", min, page)).toEqual({ ...start, width: 384 });
    expect(keyboardResize({ ...start, width: 370 }, "ArrowLeft", min, page).width).toBe(360);
    expect(keyboardResize(start, "ArrowUp", min, page)).toEqual({ ...start, height: 284 });
    expect(isArrowKey("ArrowLeft")).toBe(true);
    expect(isArrowKey("Enter")).toBe(false);
  });

  it("produces geometry the core accepts", () => {
    expect(roundRect({ x: 1.6, y: -1.4, width: 0, height: 1e9 })).toEqual({
      x: 2,
      y: -1,
      width: 1,
      height: MAX_WINDOW_EDGE,
    });
    expect(isValidGeometry(normal({ x: 10.2, y: 3.7, width: 300.5, height: 200.1 }))).toBe(true);
    expect(fitSize({ width: 10, height: 10 }, min, page)).toEqual(min);
  });

  it("compares geometries", () => {
    const a = normal({ x: 1, y: 2, width: 300, height: 200 });
    expect(sameGeometry(a, { ...a, rect: { ...a.rect } })).toBe(true);
    expect(sameGeometry(a, { ...a, maximized: true })).toBe(false);
    expect(sameGeometry(a, { ...a, snap: "left" })).toBe(false);
    expect(sameGeometry(a, { ...a, rect: { ...a.rect, x: 2 } })).toBe(false);
    expect(sameGeometry(null, null)).toBe(true);
    expect(sameGeometry(a, null)).toBe(false);
  });

  it("never lets Shift+Right / Shift+Down shrink a window that stands off the page edge", () => {
    const wide = { width: 1600, height: 900 };
    const composerMin = { width: 520, height: 400 };
    // 820 wide at x = 900: only 700 px are on the page.
    const off = { x: 900, y: 600, width: 820, height: 720 };
    const right = keyboardResize(off, "ArrowRight", composerMin, wide);
    expect(right.width).toBe(820);
    const down = keyboardResize(off, "ArrowDown", composerMin, wide);
    expect(down.height).toBe(720);
    // Shrinking still works from there, and growing on the page still grows.
    expect(keyboardResize(off, "ArrowLeft", composerMin, wide).width).toBe(
      820 - WINDOW_KEYBOARD_STEP,
    );
    const inside = { x: 100, y: 100, width: 820, height: 600 };
    expect(keyboardResize(inside, "ArrowRight", composerMin, wide).width).toBe(
      820 + WINDOW_KEYBOARD_STEP,
    );
  });

  it("centres a window over the rectangle of the window it belongs to", () => {
    const wide = { width: 1600, height: 900 };
    const confirm = { width: 480, height: 220 };
    const small = { width: 320, height: 160 };
    const owner = { x: 0, y: 0, width: 800, height: 900 };
    expect(centredOn(owner, confirm, small, wide)).toEqual({
      x: 160,
      y: 340,
      width: 480,
      height: 220,
    });
    // An owner partly off the page: the window stays on the page.
    const off = { x: 1400, y: 700, width: 820, height: 720 };
    const placed = centredOn(off, confirm, small, wide);
    expect(placed.x + placed.width).toBeLessThanOrEqual(wide.width);
    expect(placed.y + placed.height).toBeLessThanOrEqual(wide.height);
  });

  it("gives a new window the first cascade slot no open window of its kind uses", () => {
    const kindOf = (id: string) => id.split(":")[0] ?? id;
    let slots = cascadeSlots(new Map(), ["composer:a", "composer:b", "rules"], kindOf);
    expect([...slots]).toEqual([
      ["composer:a", 0],
      ["composer:b", 1],
      ["rules", 0],
    ]);
    // A closes, C opens: C takes A's slot, not B's (no window opens over another).
    slots = cascadeSlots(slots, ["composer:b", "rules", "composer:c"], kindOf);
    expect(slots.get("composer:b")).toBe(1);
    expect(slots.get("composer:c")).toBe(0);
    // D takes the next free slot.
    slots = cascadeSlots(slots, ["composer:b", "rules", "composer:c", "composer:d"], kindOf);
    expect(slots.get("composer:d")).toBe(2);
    // Nothing changed: the same map.
    const order = ["composer:b", "rules", "composer:c", "composer:d"];
    expect(cascadeSlots(slots, order, kindOf)).toBe(slots);
  });
});
