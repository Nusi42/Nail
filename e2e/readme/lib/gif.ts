/**
 * Animated GIFs for the README (ADR 0197) from Playwright screenshots:
 * every frame gets its own palette (`gifenc`), and the pixels that did not
 * change since the previous frame are written as the transparent colour on
 * top of it, so a mostly still UI stays small. Only erasable TypeScript, so
 * `node --test` runs `gif.test.ts` directly.
 */
import gifenc from "gifenc";
import { decodePng, type Image } from "../../visual/lib/png.ts";

// gifenc is CommonJS without detectable named exports: Node only offers the default.
const { GIFEncoder, applyPalette, quantize } = gifenc;

/**
 * A frame: a PNG screenshot or decoded pixels (e.g. a morph step); every
 * frame of a GIF has the same size. `delay` is how long it shows, in
 * milliseconds (GIFs count in 10 ms steps).
 */
export interface PngFrame {
  png: Uint8Array;
  delay: number;
}
export type Frame = PngFrame | { image: Image; delay: number };

/** The palette slot of "unchanged since the previous frame". */
export const UNCHANGED = 255;
/** GIF palettes hold a power of two of colours; the last one is `UNCHANGED`. */
const PALETTE_SIZE = 256;

/** Per pixel: whether it differs from the previous frame (every pixel of the first). */
export function changedPixels(image: Image, previous: Image | undefined): Uint8Array {
  const changed = new Uint8Array(image.width * image.height);
  for (let i = 0; i < changed.length; i++) {
    const at = i * 4;
    const same =
      previous !== undefined &&
      previous.data[at] === image.data[at] &&
      previous.data[at + 1] === image.data[at + 1] &&
      previous.data[at + 2] === image.data[at + 2];
    changed[i] = same ? 0 : 1;
  }
  return changed;
}

/** The RGBA values of the changed pixels only, what the palette is built from. */
function changedColours(image: Image, changed: Uint8Array): Uint8Array {
  let count = 0;
  for (const c of changed) count += c;
  const colours = new Uint8Array(Math.max(count, 1) * 4);
  let n = 0;
  for (let i = 0; i < changed.length; i++) {
    if (!changed[i]) continue;
    colours.set(image.data.subarray(i * 4, i * 4 + 4), n * 4);
    n++;
  }
  return colours;
}

/** Encodes the frames as a GIF that loops forever. */
export function encodeGif(frames: readonly Frame[]): Uint8Array {
  if (frames.length === 0) throw new Error("a GIF needs at least one frame");
  const gif = GIFEncoder();
  let previous: Image | undefined;
  for (const frame of frames) {
    const image = "png" in frame ? decodePng(frame.png) : frame.image;
    if (previous && (previous.width !== image.width || previous.height !== image.height)) {
      throw new Error("every frame of a GIF has the same size");
    }
    const changed = changedPixels(image, previous);
    const colours = quantize(changedColours(image, changed), UNCHANGED, { format: "rgb565" });
    const index = applyPalette(image.data, colours, "rgb565");
    for (let i = 0; i < changed.length; i++) if (!changed[i]) index[i] = UNCHANGED;
    const palette = [...colours];
    while (palette.length < PALETTE_SIZE) palette.push([0, 0, 0]);
    gif.writeFrame(index, image.width, image.height, {
      palette,
      delay: frame.delay,
      transparent: previous !== undefined,
      transparentIndex: UNCHANGED,
      // 1 = keep this frame as the background of the next one.
      dispose: 1,
    });
    previous = image;
  }
  gif.finish();
  return gif.bytes();
}
