/**
 * Just enough PNG to compare two screenshots pixel by pixel (8-bit RGB or
 * RGBA, not interlaced — what Chromium writes) with `node:zlib`, so the
 * harness self-check can tell a real difference from a few anti-aliased
 * pixels without another dependency. Only erasable TypeScript, so
 * `node --test` runs `png.test.ts` directly.
 */
import { inflateSync } from "node:zlib";

export interface Image {
  width: number;
  height: number;
  /** RGBA, 4 bytes per pixel, row by row. */
  data: Uint8Array;
}

const SIGNATURE = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

function paeth(a: number, b: number, c: number): number {
  const p = a + b - c;
  const pa = Math.abs(p - a);
  const pb = Math.abs(p - b);
  const pc = Math.abs(p - c);
  if (pa <= pb && pa <= pc) return a;
  return pb <= pc ? b : c;
}

/** Decodes an 8-bit RGB / RGBA PNG into RGBA pixels. */
export function decodePng(png: Uint8Array): Image {
  const buf = Buffer.from(png.buffer, png.byteOffset, png.byteLength);
  if (buf.length < 8 || SIGNATURE.some((b, i) => buf[i] !== b)) throw new Error("not a PNG");
  let width = 0;
  let height = 0;
  let colorType = -1;
  const idat: Buffer[] = [];
  for (let at = 8; at + 8 <= buf.length;) {
    const length = buf.readUInt32BE(at);
    const type = buf.toString("latin1", at + 4, at + 8);
    const body = buf.subarray(at + 8, at + 8 + length);
    if (type === "IHDR") {
      width = body.readUInt32BE(0);
      height = body.readUInt32BE(4);
      const depth = body[8];
      colorType = body[9] ?? -1;
      const interlace = body[12];
      if (depth !== 8 || (colorType !== 2 && colorType !== 6) || interlace !== 0) {
        throw new Error(
          `unsupported PNG (depth ${String(depth)}, colour type ${String(colorType)}, interlace ${String(interlace)})`,
        );
      }
    } else if (type === "IDAT") {
      idat.push(body);
    } else if (type === "IEND") {
      break;
    }
    at += 12 + length;
  }
  if (colorType === -1) throw new Error("PNG without IHDR");
  const channels = colorType === 6 ? 4 : 3;
  const stride = width * channels;
  const raw = inflateSync(Buffer.concat(idat));
  if (raw.length < height * (stride + 1)) throw new Error("truncated PNG data");
  const pixels = new Uint8Array(height * stride);
  for (let y = 0; y < height; y++) {
    const filter = raw[y * (stride + 1)];
    const src = y * (stride + 1) + 1;
    const row = y * stride;
    for (let x = 0; x < stride; x++) {
      const value = raw[src + x] ?? 0;
      const left = x >= channels ? (pixels[row + x - channels] ?? 0) : 0;
      const up = y > 0 ? (pixels[row - stride + x] ?? 0) : 0;
      const upLeft = y > 0 && x >= channels ? (pixels[row - stride + x - channels] ?? 0) : 0;
      let out: number;
      switch (filter) {
        case 0:
          out = value;
          break;
        case 1:
          out = value + left;
          break;
        case 2:
          out = value + up;
          break;
        case 3:
          out = value + ((left + up) >> 1);
          break;
        case 4:
          out = value + paeth(left, up, upLeft);
          break;
        default:
          throw new Error(`unknown PNG filter ${String(filter)}`);
      }
      pixels[row + x] = out & 0xff;
    }
  }
  if (channels === 4) return { width, height, data: pixels };
  const data = new Uint8Array(width * height * 4);
  for (let p = 0, q = 0; p < pixels.length; p += 3, q += 4) {
    data[q] = pixels[p] ?? 0;
    data[q + 1] = pixels[p + 1] ?? 0;
    data[q + 2] = pixels[p + 2] ?? 0;
    data[q + 3] = 255;
  }
  return { width, height, data };
}

export interface PixelDiff {
  /** Pixels whose colour differs. */
  pixels: number;
  /** Share of all pixels (1 when the sizes differ). */
  ratio: number;
  /** Bounding box `[x0, y0, x1, y1]` of the differing pixels, `null` when none. */
  box: [number, number, number, number] | null;
}

/** Compares two screenshots exactly, pixel by pixel. */
export function diffPngs(a: Uint8Array, b: Uint8Array): PixelDiff {
  const ia = decodePng(a);
  const ib = decodePng(b);
  if (ia.width !== ib.width || ia.height !== ib.height) {
    return { pixels: Math.max(ia.width * ia.height, ib.width * ib.height), ratio: 1, box: null };
  }
  let pixels = 0;
  let box: [number, number, number, number] | null = null;
  for (let i = 0; i < ia.data.length; i += 4) {
    if (
      ia.data[i] !== ib.data[i] ||
      ia.data[i + 1] !== ib.data[i + 1] ||
      ia.data[i + 2] !== ib.data[i + 2] ||
      ia.data[i + 3] !== ib.data[i + 3]
    ) {
      pixels++;
      const x = (i / 4) % ia.width;
      const y = Math.floor(i / 4 / ia.width);
      box = box
        ? [Math.min(box[0], x), Math.min(box[1], y), Math.max(box[2], x), Math.max(box[3], y)]
        : [x, y, x, y];
    }
  }
  return { pixels, ratio: pixels / (ia.width * ia.height), box };
}
