// node --test e2e/perf/run-perf.test.mjs — the perf runner's arguments and
// the budgets file's shape (the Rust side checks the metrics in depth).
import assert from "node:assert/strict";
import { test } from "node:test";
import { readJson } from "../../scripts/lib/json.mjs";
import { parseArgs } from "./run-perf.mjs";

test("arguments", () => {
  assert.deepEqual(parseArgs([]), { corpus: "standard", tolerant: false });
  assert.deepEqual(parseArgs(["--corpus", "small", "--tolerant"]), {
    corpus: "small",
    tolerant: true,
  });
  assert.throws(() => parseArgs(["--corpus", "huge"]), /unknown corpus/);
  assert.throws(() => parseArgs(["--fast"]), /unknown argument/);
});

test("budgets cover the UI metrics the scenarios record", () => {
  const budgets = /** @type {{ budgets: { metric: string }[], "ci-tolerance": number }} */ (
    readJson(new URL("./budgets.json", import.meta.url))
  );
  const metrics = budgets.budgets.map((b) => b.metric);
  for (const m of ["first-render-ms", "scroll-frame-p95-ms", "scroll-long-frames"]) {
    assert.ok(metrics.includes(m), m);
  }
  assert.equal(new Set(metrics).size, metrics.length);
  assert.ok(budgets["ci-tolerance"] >= 1);
});
