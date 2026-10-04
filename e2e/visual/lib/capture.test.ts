// node --test e2e/visual/lib/*.test.ts — settling captures and the manifest.
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { test } from "node:test";
import {
  captureStable,
  comparisonStatus,
  manifestFile,
  writeManifestEntry,
  type ManifestEntry,
} from "./capture.ts";
import { actualDir, buildBase, manifestDir, repoRoot, resultsDir } from "./paths.ts";

function shooter(frames: string[]) {
  let i = 0;
  const calls = { shots: 0, sleeps: [] as number[] };
  const shoot = () => {
    calls.shots++;
    return Promise.resolve(Buffer.from(frames[Math.min(i++, frames.length - 1)] ?? ""));
  };
  const sleep = (ms: number) => {
    calls.sleeps.push(ms);
    return Promise.resolve();
  };
  return { shoot, sleep, calls };
}

test("a capture is taken once two consecutive screenshots are identical", async () => {
  const s = shooter(["loading", "half", "done", "done", "never"]);
  const result = await captureStable(s.shoot, { sleep: s.sleep, intervalMs: 10 });
  assert.equal(result.stable, true);
  assert.equal(result.attempts, 4);
  assert.equal(result.png.toString(), "done");
  assert.deepEqual(s.calls.sleeps, [10, 10, 10]);
});

test("a screen that never settles is reported, with the last screenshot", async () => {
  let n = 0;
  const result = await captureStable(() => Promise.resolve(Buffer.from(`frame ${String(n++)}`)), {
    attempts: 3,
    sleep: async () => {
      /* no-op */
    },
  });
  assert.equal(result.stable, false);
  assert.equal(result.attempts, 3);
  assert.equal(result.png.toString(), "frame 2");
  // At least two screenshots, whatever is asked.
  const once = await captureStable(() => Promise.resolve(Buffer.from("x")), {
    attempts: 1,
    sleep: async () => {
      /* no-op */
    },
  });
  assert.deepEqual([once.stable, once.attempts], [true, 2]);
});

test("manifest entries get one file each, safe names, and round-trip", () => {
  const dir = mkdtempSync(path.join(tmpdir(), "nail-visual-manifest-"));
  try {
    const entry: ManifestEntry = {
      variant: "high-contrast-compact",
      theme: "high-contrast",
      density: "compact",
      screen: "main",
      status: "changed",
      baseline: "C:/b.png",
      actual: "C:/a.png",
      diff: "C:/d.png",
      message: "12 pixels differ",
    };
    const file = writeManifestEntry(dir, entry);
    assert.equal(path.basename(file), "high-contrast-compact.main.json");
    assert.deepEqual(JSON.parse(readFileSync(file, "utf8")), entry);
    writeManifestEntry(dir, {
      ...entry,
      variant: "sage-comfortable",
      screen: null,
      status: "skipped",
    });
    assert.deepEqual(readdirSync(dir).sort(), [
      "high-contrast-compact.main.json",
      "sage-comfortable._variant.json",
    ]);
    assert.equal(
      path.basename(manifestFile(dir, { variant: "a/b", screen: "c d" })),
      "a_b.c_d.json",
    );
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("comparison outcomes map to statuses", () => {
  assert.equal(comparisonStatus(false, false), "new");
  assert.equal(comparisonStatus(false, true), "new");
  assert.equal(comparisonStatus(true, true), "passed");
  assert.equal(comparisonStatus(true, false), "changed");
});

test("output goes to the checkout's build directory, never tracked files", () => {
  const env = { LOCALAPPDATA: path.join("C:", "Users", "x", "AppData", "Local") };
  assert.equal(buildBase(env), path.join(repoRoot, ".nail-build"));
  assert.equal(resultsDir(env), path.join(repoRoot, ".nail-build", "visual"));
  assert.equal(manifestDir({ NAIL_BUILD_DIR: "/b" }), path.join("/b", "visual", "manifest"));
  assert.equal(actualDir({ NAIL_VISUAL_RESULTS: "/r" }), path.join("/r", "actual"));
  assert.equal(buildBase({ NAIL_BUILD_DIR: "/b" }), "/b");
});
