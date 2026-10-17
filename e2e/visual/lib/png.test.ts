// node --test e2e/visual/lib/*.test.ts — the PNG decoder (every filter
// type, RGB and RGBA) and the pixel comparison of the harness self-check.
import assert from "node:assert/strict";
import { test } from "node:test";
import { decodePng, diffPngs } from "./png.ts";
import { tolerance } from "./paths.ts";
import { encodePng, picture } from "./png-fixture.ts";

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
    b[(y * 20 + x) * 4] = ((b[(y * 20 + x) * 4] ?? 0) + 1) % 256;
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
