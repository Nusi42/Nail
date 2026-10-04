// node --test scripts/visual-review/ — the review page generator: manifest
// reading and ordering, summaries, orphaned baselines, the page itself.
import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { test } from "node:test";
import {
  buildReview,
  compareEntries,
  escapeHtml,
  orphanBaselines,
  parseArgs,
  readManifest,
  renderHtml,
  repoRoot,
  STATUSES,
  summarize,
} from "./generate.mjs";
import { readJson } from "../lib/json.mjs";

function tempDir() {
  return mkdtempSync(path.join(tmpdir(), "nail-visual-review-test-"));
}

/**
 * @param {Partial<import("./generate.mjs").Entry> & Record<string, unknown>} overrides
 * @returns {import("./generate.mjs").Entry}
 */
function entry(overrides) {
  return {
    variant: "frost-comfortable",
    theme: "frost",
    density: "comfortable",
    screen: "main",
    status: "passed",
    baseline: null,
    actual: null,
    diff: null,
    message: null,
    ...overrides,
  };
}

/**
 * @param {string} dir
 * @param {unknown[]} entries
 */
function writeEntries(dir, entries) {
  mkdirSync(dir, { recursive: true });
  entries.forEach((e, i) => {
    writeFileSync(path.join(dir, `${String(i)}.json`), JSON.stringify(e));
  });
}

test("arguments default to the checkout's build directory, never tracked files", () => {
  const options = parseArgs([], { LOCALAPPDATA: "C:\\Users\\x\\AppData\\Local" });
  assert.equal(options.results, path.join(repoRoot, ".nail-build", "visual"));
  assert.equal(options.out, path.join(options.results, "review"));
  assert.ok(options.baselines.endsWith(path.join("e2e", "visual", "baselines", process.platform)));
  const env = { NAIL_BUILD_DIR: "/b", NAIL_VISUAL_BASELINES: "/base" };
  assert.equal(parseArgs([], env).results, path.join("/b", "visual"));
  assert.equal(parseArgs([], env).baselines, path.join(path.resolve("/base"), process.platform));
  assert.equal(parseArgs([], { NAIL_VISUAL_RESULTS: "/r" }).out, path.join("/r", "review"));
  const explicit = parseArgs(["--results", "/x", "--out", "/y", "--baselines", "/z"], {});
  assert.deepEqual(explicit, {
    results: path.resolve("/x"),
    out: path.resolve("/y"),
    baselines: path.resolve("/z"),
  });
  assert.throws(() => parseArgs(["--out"], {}), /--out needs a value/);
  assert.throws(() => parseArgs(["--serve"], {}), /unknown argument --serve/);
});

test("the manifest is validated and ordered: current, gallery order, densities, screens", () => {
  const dir = tempDir();
  try {
    assert.deepEqual(readManifest(path.join(dir, "missing")), []);
    writeEntries(dir, [
      entry({
        variant: "graphite-compact",
        theme: "graphite",
        density: "compact",
        screen: "dialog",
      }),
      entry({ variant: "ocean-comfortable", theme: "ocean", screen: "main" }),
      entry({ screen: "settings" }),
      entry({ variant: "graphite-compact", theme: "graphite", density: "compact", screen: "main" }),
      entry({ variant: "frost-compact", density: "compact", screen: "main" }),
      entry({ variant: "current", theme: "default", screen: "zeta", status: "captured" }),
      entry({ screen: "main", extra: "ignored" }),
      entry({ variant: "sage-comfortable", theme: "sage", screen: null, status: "skipped" }),
    ]);
    const ids = readManifest(dir).map((e) => `${e.variant}/${String(e.screen)}`);
    assert.deepEqual(ids, [
      "current/zeta",
      "frost-comfortable/main",
      "frost-comfortable/settings",
      "frost-compact/main",
      "sage-comfortable/null",
      "graphite-compact/main",
      "graphite-compact/dialog",
      "ocean-comfortable/main",
    ]);
    assert.equal("extra" in (readManifest(dir)[1] ?? {}), false);
    writeFileSync(path.join(dir, "z.json"), JSON.stringify(entry({ status: "fine" })));
    assert.throws(() => readManifest(dir), /unknown status fine/);
    writeFileSync(path.join(dir, "z.json"), JSON.stringify({ variant: "x" }));
    assert.throws(() => readManifest(dir), /theme is missing/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
  assert.equal(compareEntries(entry({}), entry({})), 0);
});

test("the summary counts every status", () => {
  const counts = summarize([entry({}), entry({ status: "changed" }), entry({ status: "changed" })]);
  assert.deepEqual(Object.keys(counts), STATUSES);
  assert.equal(counts.changed, 2);
  assert.equal(counts.passed, 1);
  assert.equal(counts.new, 0);
});

test("baselines of captured variants without a screen in the run are listed", () => {
  const dir = tempDir();
  try {
    for (const file of [
      "frost-comfortable/main.png",
      "frost-comfortable/old.png",
      "sage-compact/main.png",
    ]) {
      mkdirSync(path.dirname(path.join(dir, file)), { recursive: true });
      writeFileSync(path.join(dir, file), "png");
    }
    writeFileSync(path.join(dir, "frost-comfortable", "notes.txt"), "not a baseline");
    const entries = [
      entry({}),
      entry({
        variant: "sage-compact",
        theme: "sage",
        density: "compact",
        screen: null,
        status: "skipped",
      }),
    ];
    // sage-compact was not captured (skipped), so its baselines are not judged.
    assert.deepEqual(orphanBaselines(dir, entries), ["frost-comfortable/old"]);
    assert.deepEqual(orphanBaselines(path.join(dir, "none"), entries), []);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("HTML is escaped", () => {
  assert.equal(
    escapeHtml(`<a href="x">&'</a>`),
    "&lt;a href=&quot;x&quot;&gt;&amp;&#39;&lt;/a&gt;",
  );
});

test("the page shows baseline, actual and difference side by side, skips, and copies images", () => {
  const dir = tempDir();
  try {
    const results = path.join(dir, "results");
    const images = path.join(dir, "src");
    mkdirSync(images, { recursive: true });
    for (const name of ["b.png", "a.png", "d.png", "a2.png"])
      writeFileSync(path.join(images, name), name);
    writeEntries(path.join(results, "manifest"), [
      entry({
        status: "changed",
        baseline: path.join(images, "b.png"),
        actual: path.join(images, "a.png"),
        diff: path.join(images, "d.png"),
        message: "812 pixels <differ>",
      }),
      entry({ screen: "composer", status: "new", actual: path.join(images, "a2.png") }),
      entry({
        screen: "menu",
        status: "error",
        actual: path.join(images, "gone.png"),
        message: "menu not found",
      }),
      entry({
        variant: "sage-comfortable",
        theme: "sage",
        screen: null,
        status: "skipped",
        message: 'theme "sage" is not listed',
      }),
    ]);
    const baselines = path.join(dir, "baselines");
    mkdirSync(path.join(baselines, "frost-comfortable"), { recursive: true });
    writeFileSync(path.join(baselines, "frost-comfortable", "removed.png"), "png");
    const out = path.join(dir, "review");
    // A stale image of an earlier run disappears.
    mkdirSync(path.join(out, "images", "old"), { recursive: true });
    writeFileSync(path.join(out, "images", "old", "x.png"), "old");

    const { file, summary, orphans } = buildReview(
      { results, out, baselines },
      new Date("2026-10-03T12:00:00Z"),
    );
    assert.equal(file, path.join(out, "index.html"));
    assert.equal(summary.changed, 1);
    assert.equal(summary.new, 1);
    assert.equal(summary.error, 1);
    assert.equal(summary.skipped, 1);
    assert.deepEqual(orphans, ["frost-comfortable/removed"]);
    assert.equal(existsSync(path.join(out, "images", "old")), false);
    for (const [kind, content] of [
      ["baseline", "b.png"],
      ["actual", "a.png"],
      ["diff", "d.png"],
    ]) {
      const copied = path.join(out, "images", "frost-comfortable", `main-${kind ?? ""}.png`);
      assert.equal(readFileSync(copied, "utf8"), content);
    }
    const html = readFileSync(file, "utf8");
    assert.match(html, /<title>Visual review<\/title>/);
    assert.match(html, /Generated 2026-10-03T12:00:00.000Z/);
    assert.ok(html.includes('src="images/frost-comfortable/main-baseline.png"'));
    assert.ok(html.includes('src="images/frost-comfortable/main-actual.png"'));
    assert.ok(html.includes('src="images/frost-comfortable/main-diff.png"'));
    assert.ok(html.includes("812 pixels &lt;differ&gt;"));
    assert.ok(html.includes("No baseline yet"), "the new screen says it has no baseline");
    assert.ok(html.includes("Not captured"), "the error's missing image is a placeholder");
    assert.ok(html.includes("Skipped variants"));
    assert.ok(html.includes("theme &quot;sage&quot; is not listed"));
    assert.ok(html.includes("<code>frost-comfortable/removed</code>"));
    assert.ok(html.includes('data-filter="changed"'));
    // Self-contained: no external resources.
    assert.equal(/(src|href)="https?:/.test(html), false);
    const saved = /** @type {{ summary: Record<string, number>, entries: unknown[] }} */ (
      readJson(path.join(out, "summary.json"))
    );
    assert.equal(saved.summary.changed, 1);
    assert.equal(saved.entries.length, 4);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("an empty manifest yields a page that says so", () => {
  const html = renderHtml([], {
    summary: summarize([]),
    generatedAt: "now",
    results: "C:\\build\\visual",
  });
  assert.match(html, /The manifest is empty/);
  assert.ok(html.includes("C:\\build\\visual"));
});
