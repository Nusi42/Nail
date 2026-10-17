// Test pictures for the PNG decoder and the README GIFs: a PNG encoder that
// uses every filter type, and deterministic pseudo-random pixels.
import { crc32, deflateSync } from "node:zlib";

function chunk(type: string, body: Buffer): Buffer {
  const head = Buffer.alloc(8);
  head.writeUInt32BE(body.length, 0);
  head.write(type, 4, "latin1");
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(Buffer.concat([Buffer.from(type, "latin1"), body])) >>> 0, 0);
  return Buffer.concat([head, body, crc]);
}

function paeth(a: number, b: number, c: number): number {
  const p = a + b - c;
  const [pa, pb, pc] = [Math.abs(p - a), Math.abs(p - b), Math.abs(p - c)];
  if (pa <= pb && pa <= pc) return a;
  return pb <= pc ? b : c;
}

/** Encodes pixels (`channels` bytes each), row `y` with filter `y % 5`, split over two IDATs. */
export function encodePng(
  width: number,
  height: number,
  pixels: number[],
  channels: 3 | 4,
): Buffer {
  const stride = width * channels;
  const raw: number[] = [];
  for (let y = 0; y < height; y++) {
    const filter = y % 5;
    raw.push(filter);
    for (let x = 0; x < stride; x++) {
      const v = pixels[y * stride + x] ?? 0;
      const left = x >= channels ? (pixels[y * stride + x - channels] ?? 0) : 0;
      const up = y > 0 ? (pixels[(y - 1) * stride + x] ?? 0) : 0;
      const upLeft = y > 0 && x >= channels ? (pixels[(y - 1) * stride + x - channels] ?? 0) : 0;
      const predicted = [0, left, up, (left + up) >> 1, paeth(left, up, upLeft)][filter] ?? 0;
      raw.push((v - predicted + 256) & 0xff);
    }
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8;
  ihdr[9] = channels === 4 ? 6 : 2;
  const data = deflateSync(Buffer.from(raw));
  const half = Math.floor(data.length / 2);
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("tEXt", Buffer.from("Software\0test", "latin1")),
    chunk("IDAT", data.subarray(0, half)),
    chunk("IDAT", data.subarray(half)),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

/** A deterministic pseudo-random picture. */
export function picture(width: number, height: number, channels: 3 | 4, seed = 7): number[] {
  const out: number[] = [];
  let s = seed;
  for (let i = 0; i < width * height * channels; i++) {
    s = (s * 1103515245 + 12345) & 0x7fffffff;
    out.push(s % 256);
  }
  return out;
}
