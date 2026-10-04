/**
 * The window manager's geometry (M5c WP171, ADR 0116): pure functions over
 * page rectangles, so moving, resizing, snapping, maximizing and keeping
 * windows on the page are tested without a browser.
 *
 * Terms:
 * - the *viewport* is the page the windows live on (the window layer fills
 *   it: `window.innerWidth` × `window.innerHeight`);
 * - a window's *geometry* (`WindowGeometry`, remembered per kind) is its
 *   restored rectangle plus maximized / snapped; the *shown* rectangle is
 *   derived from it and the viewport (`shownRect`), so a page that shrinks
 *   and grows again gives the window back its old place;
 * - at least `WINDOW_MIN_VISIBLE` pixels of a window stay on the page, and
 *   its title bar never leaves the top edge.
 */
import type { WindowGeometry, WindowRect, WindowSize, WindowSnap } from "../../api/generated";
import {
  MAX_WINDOW_COORDINATE,
  MAX_WINDOW_EDGE,
  WINDOW_CASCADE_OFFSET,
  WINDOW_KEYBOARD_STEP,
  WINDOW_MIN_VISIBLE,
} from "../../state/windows";

/** The page size in CSS pixels. */
export interface Viewport {
  width: number;
  height: number;
}

/** A resize handle: the edge or corner it moves. */
export type ResizeEdge = "n" | "s" | "e" | "w" | "ne" | "nw" | "se" | "sw";

/** The eight handles, in DOM order. */
export const RESIZE_EDGES: readonly ResizeEdge[] = ["n", "e", "s", "w", "ne", "se", "sw", "nw"];

/** How close (px) to the left / right page edge the pointer snaps a dragged window. */
export const SNAP_EDGE = 24;

/** How far (px) the pointer moves before a maximized / snapped window comes loose. */
export const DRAG_THRESHOLD = 4;

/** Delay (ms) before a changed geometry is remembered (drags and key repeats coalesce). */
export const SAVE_DELAY_MS = 400;

const clamp = (value: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, value));

/** Whole pixels within the remembered-geometry limits. */
export function roundRect(rect: WindowRect): WindowRect {
  const edge = (v: number) => clamp(Math.round(v), 1, MAX_WINDOW_EDGE);
  const coord = (v: number) => clamp(Math.round(v), -MAX_WINDOW_COORDINATE, MAX_WINDOW_COORDINATE);
  return { x: coord(rect.x), y: coord(rect.y), width: edge(rect.width), height: edge(rect.height) };
}

/** A size no smaller than `min` and no larger than the page (the page wins). */
export function fitSize(size: WindowSize, min: WindowSize, viewport: Viewport): WindowSize {
  return {
    width: Math.max(1, Math.min(Math.max(size.width, min.width), viewport.width)),
    height: Math.max(1, Math.min(Math.max(size.height, min.height), viewport.height)),
  };
}

/**
 * Keeps a window reachable: `WINDOW_MIN_VISIBLE` pixels stay on the page
 * horizontally and at the bottom, and the title bar stays below the top
 * edge.
 */
export function keepOnPage(rect: WindowRect, viewport: Viewport): WindowRect {
  const visible = Math.min(WINDOW_MIN_VISIBLE, rect.width, viewport.width);
  const x = clamp(rect.x, visible - rect.width, Math.max(0, viewport.width - visible));
  const y = clamp(
    rect.y,
    0,
    Math.max(0, viewport.height - Math.min(WINDOW_MIN_VISIBLE, rect.height)),
  );
  return { ...rect, x, y };
}

/** A rectangle fully on the page where it fits (used when a window opens). */
export function insidePage(rect: WindowRect, viewport: Viewport): WindowRect {
  return {
    ...rect,
    x: clamp(rect.x, 0, Math.max(0, viewport.width - rect.width)),
    y: clamp(rect.y, 0, Math.max(0, viewport.height - rect.height)),
  };
}

/**
 * Where a window opens without remembered geometry: the spec's size,
 * centred, moved down and right by `WINDOW_CASCADE_OFFSET` per window of
 * the same kind already open (`cascade`).
 */
export function defaultRect(
  size: WindowSize,
  min: WindowSize,
  viewport: Viewport,
  cascade = 0,
): WindowRect {
  const fitted = fitSize(size, min, viewport);
  const offset = cascade * WINDOW_CASCADE_OFFSET;
  return roundRect(
    insidePage(
      {
        x: (viewport.width - fitted.width) / 2 + offset,
        y: (viewport.height - fitted.height) / 2 + offset,
        ...fitted,
      },
      viewport,
    ),
  );
}

/**
 * Where a window that belongs to another one opens (a confirmation of a
 * composer): the spec's size, centred over `anchor` (the owner's shown
 * rectangle), fully on the page where it fits — like an OS dialog over its
 * owner window.
 */
export function centredOn(
  anchor: WindowRect,
  size: WindowSize,
  min: WindowSize,
  viewport: Viewport,
): WindowRect {
  const fitted = fitSize(size, min, viewport);
  return roundRect(
    insidePage(
      {
        x: anchor.x + (anchor.width - fitted.width) / 2,
        y: anchor.y + (anchor.height - fitted.height) / 2,
        ...fitted,
      },
      viewport,
    ),
  );
}

/**
 * The cascade slot of every window in `order` (opening order): a window
 * keeps the slot it opened with; a new one takes the lowest slot no open
 * window of its kind uses, so it never opens exactly over one of them
 * (open A and B, close A, open C: C takes A's place, not B's).
 * `previous` itself when nothing changed.
 */
export function cascadeSlots(
  previous: ReadonlyMap<string, number>,
  order: readonly string[],
  kindOf: (id: string) => string,
): ReadonlyMap<string, number> {
  const next = new Map<string, number>();
  for (const id of order) {
    const slot = previous.get(id);
    if (slot !== undefined) next.set(id, slot);
  }
  for (const id of order) {
    if (next.has(id)) continue;
    const kind = kindOf(id);
    const used = new Set(
      [...next].filter(([other]) => kindOf(other) === kind).map(([, slot]) => slot),
    );
    let slot = 0;
    while (used.has(slot)) slot += 1;
    next.set(id, slot);
  }
  const same = next.size === previous.size && [...next].every(([id, s]) => previous.get(id) === s);
  return same ? previous : next;
}

/**
 * The geometry a window opens with: the remembered one (cascaded when
 * another window of the kind is open), else the default.
 */
export function initialGeometry(
  remembered: WindowGeometry | null,
  size: WindowSize,
  min: WindowSize,
  viewport: Viewport,
  cascade = 0,
): WindowGeometry {
  if (remembered === null) {
    return { rect: defaultRect(size, min, viewport, cascade), maximized: false, snap: null };
  }
  const offset = cascade * WINDOW_CASCADE_OFFSET;
  const fitted = fitSize(remembered.rect, min, viewport);
  const rect = roundRect(
    insidePage(
      { x: remembered.rect.x + offset, y: remembered.rect.y + offset, ...fitted },
      viewport,
    ),
  );
  // A further window of the kind opens beside the first, not over it.
  if (cascade > 0) return { rect, maximized: false, snap: null };
  return {
    rect,
    maximized: remembered.maximized,
    snap: remembered.maximized ? null : remembered.snap,
  };
}

/** The half of the page a snapped window fills (at least its minimum width). */
export function snapRect(snap: WindowSnap, min: WindowSize, viewport: Viewport): WindowRect {
  const width = Math.min(viewport.width, Math.max(Math.floor(viewport.width / 2), min.width));
  return {
    x: snap === "left" ? 0 : viewport.width - width,
    y: 0,
    width,
    height: viewport.height,
  };
}

/** The whole page. */
export function maximizedRect(viewport: Viewport): WindowRect {
  return { x: 0, y: 0, width: viewport.width, height: viewport.height };
}

/** Whether `rect` lies wholly on a page of size `viewport`. */
export function fitsPage(rect: WindowRect, viewport: Viewport): boolean {
  return (
    rect.x >= 0 &&
    rect.y >= 0 &&
    rect.x + rect.width <= viewport.width &&
    rect.y + rect.height <= viewport.height
  );
}

/**
 * The rectangle a window is shown at.
 *
 * `placedOnPage` (P1 WP191): the window lay wholly on the page when it was
 * placed (opened, moved or resized). Such a window stays wholly on a page
 * that shrinks — moved in from the right and bottom edges as far as needed,
 * so its title bar and buttons stay reachable — and gets its old place back
 * when the page grows again. A window the user dragged partly off the page
 * keeps only `WINDOW_MIN_VISIBLE` pixels on it, as before.
 */
export function shownRect(
  geometry: WindowGeometry,
  min: WindowSize,
  viewport: Viewport,
  placedOnPage = false,
): WindowRect {
  if (geometry.maximized) return maximizedRect(viewport);
  if (geometry.snap !== null) return snapRect(geometry.snap, min, viewport);
  const size = fitSize(geometry.rect, min, viewport);
  const rect = { x: geometry.rect.x, y: geometry.rect.y, ...size };
  return placedOnPage ? insidePage(rect, viewport) : keepOnPage(rect, viewport);
}

/** A normal geometry at `rect`. */
export function normal(rect: WindowRect): WindowGeometry {
  return { rect: roundRect(rect), maximized: false, snap: null };
}

/** Moves `start` by (dx, dy), keeping it on the page. */
export function moveRect(
  start: WindowRect,
  dx: number,
  dy: number,
  viewport: Viewport,
): WindowRect {
  return roundRect(keepOnPage({ ...start, x: start.x + dx, y: start.y + dy }, viewport));
}

/**
 * Resizes `start` from a handle by (dx, dy): the opposite edges stay put,
 * the size stays at least `min`, and the moved edges stay on the page.
 */
export function resizeRect(
  start: WindowRect,
  edge: ResizeEdge,
  dx: number,
  dy: number,
  min: WindowSize,
  viewport: Viewport,
): WindowRect {
  let { x, y, width, height } = start;
  if (edge.includes("e")) {
    const max = Math.max(min.width, viewport.width - start.x);
    width = clamp(start.width + dx, min.width, max);
  } else if (edge.includes("w")) {
    const right = start.x + start.width;
    const max = Math.max(min.width, right);
    width = clamp(start.width - dx, min.width, max);
    x = right - width;
  }
  if (edge.includes("s")) {
    const max = Math.max(min.height, viewport.height - start.y);
    height = clamp(start.height + dy, min.height, max);
  } else if (edge.startsWith("n")) {
    const bottom = start.y + start.height;
    const max = Math.max(min.height, bottom);
    height = clamp(start.height - dy, min.height, max);
    y = bottom - height;
  }
  return roundRect({ x, y, width, height });
}

/** Which half a window dragged with the pointer at `clientX` snaps to, if any. */
export function snapTarget(clientX: number, viewport: Viewport): WindowSnap | null {
  if (clientX <= SNAP_EDGE) return "left";
  if (clientX >= viewport.width - SNAP_EDGE) return "right";
  return null;
}

/**
 * The normal rectangle of a maximized / snapped window that comes loose
 * under the pointer: its restored size, placed so the pointer keeps its
 * relative place in the title bar (grabbed at `grab`, now at `pointer`).
 */
export function looseRect(
  shown: WindowRect,
  restored: WindowRect,
  grab: { x: number; y: number },
  pointer: { x: number; y: number },
  min: WindowSize,
  viewport: Viewport,
): WindowRect {
  const size = fitSize(restored, min, viewport);
  const ratio = shown.width > 0 ? (grab.x - shown.x) / shown.width : 0.5;
  return roundRect(
    keepOnPage(
      { x: pointer.x - ratio * size.width, y: pointer.y - (grab.y - shown.y), ...size },
      viewport,
    ),
  );
}

/** The arrow keys. */
export type ArrowKey = "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown";

export function isArrowKey(key: string): key is ArrowKey {
  return key === "ArrowLeft" || key === "ArrowRight" || key === "ArrowUp" || key === "ArrowDown";
}

const ARROWS: Readonly<Record<ArrowKey, readonly [number, number]>> = {
  ArrowLeft: [-1, 0],
  ArrowRight: [1, 0],
  ArrowUp: [0, -1],
  ArrowDown: [0, 1],
};

/** One keyboard move step (`WINDOW_KEYBOARD_STEP`). */
export function keyboardMove(start: WindowRect, key: ArrowKey, viewport: Viewport): WindowRect {
  const [dx, dy] = ARROWS[key];
  return moveRect(start, dx * WINDOW_KEYBOARD_STEP, dy * WINDOW_KEYBOARD_STEP, viewport);
}

/**
 * One keyboard resize step from the bottom-right corner: Right / Down grow
 * the window, Left / Up shrink it (not below `min`). Growing never makes it
 * smaller (a window standing off the right or bottom page edge keeps its
 * size).
 */
export function keyboardResize(
  start: WindowRect,
  key: ArrowKey,
  min: WindowSize,
  viewport: Viewport,
): WindowRect {
  const [dx, dy] = ARROWS[key];
  // Up to the page's right / bottom edge, but never below the current size.
  const edge = (size: number, delta: number, room: number, least: number) =>
    Math.min(MAX_WINDOW_EDGE, clamp(size + delta, least, Math.max(size, room, least)));
  return {
    ...start,
    width:
      dx === 0
        ? start.width
        : edge(start.width, dx * WINDOW_KEYBOARD_STEP, viewport.width - start.x, min.width),
    height:
      dy === 0
        ? start.height
        : edge(start.height, dy * WINDOW_KEYBOARD_STEP, viewport.height - start.y, min.height),
  };
}

/** Whether two geometries are the same (nothing to remember). */
export function sameGeometry(a: WindowGeometry | null, b: WindowGeometry | null): boolean {
  if (a === null || b === null) return a === b;
  return (
    a.maximized === b.maximized &&
    a.snap === b.snap &&
    a.rect.x === b.rect.x &&
    a.rect.y === b.rect.y &&
    a.rect.width === b.rect.width &&
    a.rect.height === b.rect.height
  );
}
