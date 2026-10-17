/**
 * The README hero's theme morph (ADR 0197): a diagonal wipe from one
 * screenshot to the next. Each step only changes a band of pixels, so the
 * GIF's frame diffing keeps the animation small. Only erasable TypeScript,
 * so `node --test` runs `morph.test.ts` directly.
 */
import type { Image } from "../../visual/lib/png.ts";

/** How far right the wipe's edge leans per row (in pixels per pixel of height). */
const SLANT = 0.5;
/** The bright edge line of the wipe, in pixels. */
const EDGE = 3;
const EDGE_COLOUR = [255, 255, 255] as const;

/**
 * `from` with `to` revealed up to `progress` (0 = only `from`, 1 = only
 * `to`), along an edge leaning from the top left to the bottom right.
 */
export function diagonalWipe(from: Image, to: Image, progress: number): Image {
  if (from.width !== to.width || from.height !== to.height) {
    throw new Error("a wipe needs two images of the same size");
  }
  const { width, height } = from;
  const span = width + height * SLANT;
  const edge = progress * (span + EDGE) - EDGE;
  const data = new Uint8Array(from.data);
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const along = x + y * SLANT;
      const at = (y * width + x) * 4;
      if (along < edge) {
        data.set(to.data.subarray(at, at + 4), at);
      } else if (along < edge + EDGE && progress > 0 && progress < 1) {
        data.set(EDGE_COLOUR, at);
        data[at + 3] = 255;
      }
    }
  }
  return { width, height, data };
}

/** The progress of each in-between step of a wipe with `steps` steps (0 and 1 excluded). */
export function wipeSteps(steps: number): number[] {
  return Array.from({ length: steps }, (_, i) => (i + 1) / (steps + 1));
}
