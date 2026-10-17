// node --test "e2e/readme/lib/*.test.ts" — the README GIF encoder (ADR 0197).
import assert from "node:assert/strict";
import { test } from "node:test";
import { decodePng } from "../../visual/lib/png.ts";
import { encodePng, picture } from "../../visual/lib/png-fixture.ts";
import { changedPixels, encodeGif } from "./gif.ts";

const WIDTH = 6;
const HEIGHT = 4;
const first = picture(WIDTH, HEIGHT, 4, 3).map((v, i) => (i % 4 === 3 ? 255 : v));
// The second frame changes one pixel only.
const second = [...first];
second.splice(4 * 5, 3, 1, 2, 3);
const png = (pixels: number[]) => encodePng(WIDTH, HEIGHT, pixels, 4);

/** The frames of a GIF: count, delays (1/100 s) and transparency flags. */
function frames(gif: Uint8Array): { delay: number; transparent: boolean }[] {
  const out: { delay: number; transparent: boolean }[] = [];
  for (let i = 0; i + 7 < gif.length; i++) {
    // Graphic control extension: 21 F9 04 <flags> <delay lo> <delay hi> <index> 00.
    if (gif[i] === 0x21 && gif[i + 1] === 0xf9 && gif[i + 2] === 4) {
      const flags = gif[i + 3] ?? 0;
      out.push({
        delay: (gif[i + 4] ?? 0) + (gif[i + 5] ?? 0) * 256,
        transparent: (flags & 1) === 1,
      });
    }
  }
  return out;
}

test("only the pixels that changed are drawn again", () => {
  const a = decodePng(png(first));
  const b = decodePng(png(second));
  assert.deepEqual([...changedPixels(a, undefined)], new Array(WIDTH * HEIGHT).fill(1));
  const changed = changedPixels(b, a);
  assert.equal(
    changed.reduce((n, c) => n + c, 0),
    1,
  );
  assert.equal(changed[5], 1);
});

test("a looping GIF89a with the frames' delays, later frames over the earlier ones", () => {
  const gif = encodeGif([
    { png: png(first), delay: 1200 },
    { png: png(second), delay: 80 },
  ]);
  assert.equal(Buffer.from(gif.subarray(0, 6)).toString("latin1"), "GIF89a");
  assert.equal(gif[6], WIDTH);
  assert.equal(gif[8], HEIGHT);
  assert.ok(Buffer.from(gif).includes(Buffer.from("NETSCAPE2.0", "latin1")));
  assert.deepEqual(frames(gif), [
    { delay: 120, transparent: false },
    { delay: 8, transparent: true },
  ]);
  assert.equal(gif.at(-1), 0x3b);
});

test("frames of different sizes and an empty list are refused", () => {
  assert.throws(() => encodeGif([]), /at least one frame/);
  const small = encodePng(2, 2, picture(2, 2, 4), 4);
  assert.throws(
    () =>
      encodeGif([
        { png: png(first), delay: 10 },
        { png: small, delay: 10 },
      ]),
    /same size/,
  );
});
