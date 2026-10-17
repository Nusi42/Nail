// node --test "e2e/readme/lib/*.test.ts" — the README hero's theme morph (ADR 0197).
import assert from "node:assert/strict";
import { test } from "node:test";
import type { Image } from "../../visual/lib/png.ts";
import { diagonalWipe, wipeSteps } from "./morph.ts";

const solid = (width: number, height: number, value: number): Image => ({
  width,
  height,
  data: new Uint8Array(width * height * 4).fill(value),
});
const at = (image: Image, x: number, y: number) => image.data[(y * image.width + x) * 4];

test("the wipe starts as the first image and ends as the second", () => {
  const a = solid(20, 10, 10);
  const b = solid(20, 10, 200);
  assert.deepEqual([...diagonalWipe(a, b, 0).data], [...a.data]);
  assert.deepEqual([...diagonalWipe(a, b, 1).data], [...b.data]);
});

test("half way the top left is new, the bottom right old, with an edge between", () => {
  const a = solid(40, 20, 10);
  const b = solid(40, 20, 200);
  const half = diagonalWipe(a, b, 0.5);
  assert.equal(at(half, 0, 0), 200);
  assert.equal(at(half, 39, 19), 10);
  assert.ok(
    [...half.data].some((v) => v === 255),
    "the edge line shows",
  );
  // The inputs stay untouched.
  assert.equal(at(a, 0, 0), 10);
});

test("steps exclude both ends; sizes must match", () => {
  assert.deepEqual(wipeSteps(3), [0.25, 0.5, 0.75]);
  assert.throws(() => diagonalWipe(solid(2, 2, 0), solid(3, 2, 0), 0.5), /same size/);
});
