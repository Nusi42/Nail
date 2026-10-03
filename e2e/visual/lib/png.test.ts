// node --test e2e/visual/lib/*.test.ts — the PNG decoder (every filter
// type, RGB and RGBA) and the pixel comparison of the harness self-check.
import assert from "node:assert/strict";
import { test } from "node:test";
import { crc32, deflateSync } from "node:zlib";
import { decodePng, diffPngs } from "./png.ts";
import { tolerance } from "./paths.ts";

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
function encodePng(width: number, height: number, pixels: number[], channels: 3 | 4): Buffer {
  const stride = width * channels;
  const raw: number[] = [];
  for (let y = 0; y < height; y++) {
    const filter = y % 5;
    raw.push(filter);
    for (let x = 0; x < stride; x++) {
      const v = pixels[y * stride + x]!;
      const left = x >= channels ? pixels[y * stride + x - channels]! : 0;
      const up = y > 0 ? pixels[(y - 1) * stride + x]! : 0;
      const upLeft = y > 0 && x >= channels ? pixels[(y - 1) * stride + x - channels]! : 0;
      const predicted = [0, left, up, (left + up) >> 1, paeth(left, up, upLeft)][filter]!;
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
function picture(width: number, height: number, channels: 3 | 4, seed = 7): number[] {
  const out: number[] = [];
  let s = seed;
  for (let i = 0; i < width * height * channels; i++) {
    s = (s * 1103515245 + 12345) & 0x7fffffff;
    out.push(s % 256);
  }
  return out;
}

test("RGBA pictures decode exactly through every filter type", () => {
  const pixels = picture(7, 10, 4);
  const image = decodePng(encodePng(7, 10, pixels, 4));
  assert.equal(image.width, 7);
  assert.equal(image.height, 10);
  assert.deepEqual([...image.data], pixels);
});

test("RGB pictures decode to opaque RGBA", () => {
  const pixels = picture(5, 6, 3);
  const image = decodePng(encodePng(5, 6, pixels, 3));
  assert.equal(image.data.length, 5 * 6 * 4);
  for (let p = 0; p < 30; p++) {
    assert.deepEqual(
      [...image.data.subarray(p * 4, p * 4 + 4)],
      [...pixels.slice(p * 3, p * 3 + 3), 255],
    );
  }
});

test("unsupported and broken files are refused", () => {
  assert.throws(() => decodePng(Buffer.from("GIF89a....")), /not a PNG/);
  const png = encodePng(2, 2, picture(2, 2, 4), 4);
  const paletted = Buffer.from(png);
  paletted[8 + 8 + 9] = 3; // IHDR colour type → palette
  assert.throws(() => decodePng(paletted), /unsupported PNG/);
});

test("pixel differences are counted with their bounding box", () => {
  const a = picture(20, 10, 4);
  const b = [...a];
  for (const [x, y] of [
    [3, 2],
    [4, 2],
    [12, 7],
  ] as const) {
    b[(y * 20 + x) * 4] = (b[(y * 20 + x) * 4]! + 1) % 256;
  }
  const same = diffPngs(encodePng(20, 10, a, 4), encodePng(20, 10, a, 4));
  assert.deepEqual(same, { pixels: 0, ratio: 0, box: null });
  const diff = diffPngs(encodePng(20, 10, a, 4), encodePng(20, 10, b, 4));
  assert.equal(diff.pixels, 3);
  assert.equal(diff.ratio, 3 / 200);
  assert.deepEqual(diff.box, [3, 2, 12, 7]);
  const sizes = diffPngs(encodePng(20, 10, a, 4), encodePng(10, 20, a, 4));
  assert.equal(sizes.ratio, 1);
});

test("the tolerance has defaults and accepts only sane overrides", () => {
  assert.deepEqual(tolerance({}), { maxDiffPixelRatio: 0.002, threshold: 0.15 });
  assert.deepEqual(tolerance({ NAIL_VISUAL_MAX_DIFF_RATIO: "0.01", NAIL_VISUAL_THRESHOLD: "0" }), {
    maxDiffPixelRatio: 0.01,
    threshold: 0,
  });
  assert.deepEqual(tolerance({ NAIL_VISUAL_MAX_DIFF_RATIO: "2", NAIL_VISUAL_THRESHOLD: "x" }), {
    maxDiffPixelRatio: 0.002,
    threshold: 0.15,
  });
});
