#!/usr/bin/env node
/**
 * Builds the review page of a visual-regression run (plan M5a WP154): one
 * static, self-contained HTML page with baseline, actual and difference
 * side by side for every screen × theme × density, so a human can accept
 * (`npm run visual:update`) or reject the changes.
 *
 * Input: the manifest the harness (`e2e/visual/`) writes, one JSON file
 * per screen in `<results>/manifest/`. Output: `<out>/index.html`, the
 * images it shows (copied into `<out>/images/`, so the page survives the
 * next run and can be uploaded as a CI artifact) and `<out>/summary.json`.
 * Both default to the build directory (`<build dir>/visual/…`), never the
 * repository. Nothing is served: open the file in a browser.
 *
 * Usage:
 *   node scripts/visual-review/generate.mjs [--results <dir>] [--out <dir>]
 *        [--baselines <dir>]
 */
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { parseJson } from "../lib/json.mjs";

/**
 * One manifest entry: a screen of a theme × density variant (or a whole
 * variant, `screen: null`, when it was skipped).
 *
 * @typedef {object} Entry
 * @property {string} variant
 * @property {string} theme
 * @property {string} density
 * @property {string | null} screen
 * @property {string} status one of {@link STATUSES}
 * @property {string | null} baseline
 * @property {string | null} actual
 * @property {string | null} diff
 * @property {string | null} message
 */

/** @typedef {{ results: string, out: string, baselines: string }} Options */

/** @typedef {Partial<Record<string, string>>} Env */

const here = path.dirname(fileURLToPath(import.meta.url));
export const repoRoot = path.resolve(here, "..", "..");

/** Statuses in the order the summary lists them (lib/capture.ts `CaptureStatus`). */
export const STATUSES = ["changed", "new", "error", "passed", "captured", "skipped"];

/** Gallery order of the built-in themes (e2e/visual/lib/matrix.ts). */
const THEME_ORDER = ["frost", "sage", "graphite", "nocturne", "high-contrast"];
const DENSITY_ORDER = ["comfortable", "compact"];
/** Screen order of e2e/visual/screens.ts. */
const SCREEN_ORDER = [
  "main",
  "composer",
  "settings",
  "calendar",
  "menu",
  "html",
  "attachment",
  "attachment-saved",
  "dialog",
];

/**
 * `<build dir>`, as e2e/support/nail.ts `buildOutputDir` computes it.
 *
 * @param {Env} [env]
 */
export function buildBase(env = process.env) {
  return env.NAIL_BUILD_DIR ?? path.join(repoRoot, ".nail-build");
}

/**
 * The command-line options (see the usage above).
 *
 * @param {string[]} argv
 * @param {Env} [env]
 * @returns {Options}
 */
export function parseArgs(argv, env = process.env) {
  const results = env.NAIL_VISUAL_RESULTS ?? path.join(buildBase(env), "visual");
  /** @type {{ results: string, out: string | null, baselines: string }} */
  const options = {
    results,
    out: null,
    baselines: path.join(
      env.NAIL_VISUAL_BASELINES
        ? path.resolve(env.NAIL_VISUAL_BASELINES)
        : path.join(repoRoot, "e2e", "visual", "baselines"),
      process.platform,
    ),
  };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i] ?? "";
    const value = () => {
      const v = argv[++i];
      if (!v) throw new Error(`${arg} needs a value`);
      return path.resolve(v);
    };
    if (arg === "--results") options.results = value();
    else if (arg === "--out") options.out = value();
    else if (arg === "--baselines") options.baselines = value();
    else throw new Error(`unknown argument ${arg}`);
  }
  return { ...options, out: options.out ?? path.join(options.results, "review") };
}

/**
 * @param {string[]} order
 * @param {string} value
 */
const rank = (order, value) => {
  const i = order.indexOf(value);
  return i === -1 ? order.length : i;
};

/**
 * Orders entries: `current` first, then themes in gallery order, densities, screens.
 *
 * @param {Entry} a
 * @param {Entry} b
 */
export function compareEntries(a, b) {
  /**
   * @param {Entry} e
   * @returns {(string | number)[]}
   */
  const key = (e) => [
    e.variant === "current" ? 0 : 1,
    rank(THEME_ORDER, e.theme),
    e.theme,
    rank(DENSITY_ORDER, e.density),
    e.screen === null ? -1 : rank(SCREEN_ORDER, e.screen),
    e.screen ?? "",
  ];
  const ka = key(a);
  const kb = key(b);
  for (let i = 0; i < ka.length; i++) {
    const [x = "", y = ""] = [ka[i], kb[i]];
    if (x < y) return -1;
    if (x > y) return 1;
  }
  return 0;
}

/**
 * A field of a manifest entry: a string, or `null` when it is missing.
 *
 * @param {Record<string, unknown>} entry
 * @param {string} field
 */
function optionalString(entry, field) {
  const value = entry[field];
  return typeof value === "string" ? value : null;
}

/**
 * Reads and validates every manifest entry of `dir` (missing dir = none).
 *
 * @param {string} dir
 * @returns {Entry[]}
 */
export function readManifest(dir) {
  if (!existsSync(dir)) return [];
  /** @type {Entry[]} */
  const entries = [];
  for (const name of readdirSync(dir)
    .filter((n) => n.endsWith(".json"))
    .sort()) {
    const parsed = parseJson(readFileSync(path.join(dir, name), "utf8"));
    const entry = typeof parsed === "object" && parsed !== null ? { ...parsed } : {};
    /** @param {string} field */
    const required = (field) => {
      const value = optionalString(entry, field);
      if (value === null) throw new Error(`${name}: ${field} is missing`);
      return value;
    };
    const [variant, theme, density, status] = ["variant", "theme", "density", "status"].map(
      required,
    );
    if (status === undefined || !STATUSES.includes(status))
      throw new Error(`${name}: unknown status ${status ?? ""}`);
    entries.push({
      variant: variant ?? "",
      theme: theme ?? "",
      density: density ?? "",
      screen: optionalString(entry, "screen"),
      status,
      baseline: optionalString(entry, "baseline"),
      actual: optionalString(entry, "actual"),
      diff: optionalString(entry, "diff"),
      message: optionalString(entry, "message"),
    });
  }
  return entries.sort(compareEntries);
}

/**
 * Count per status (every status present, 0 when none).
 *
 * @param {Entry[]} entries
 * @returns {Record<string, number>}
 */
export function summarize(entries) {
  /** @type {Record<string, number>} */
  const counts = Object.fromEntries(STATUSES.map((s) => [s, 0]));
  for (const e of entries) counts[e.status] = (counts[e.status] ?? 0) + 1;
  return counts;
}

/**
 * Baselines (`<variant>/<screen>.png` under `baselinesDir`) of variants
 * this run captured but whose screen it did not: screens that were
 * removed or renamed, whose baselines can be deleted.
 *
 * @param {string} baselinesDir
 * @param {Entry[]} entries
 * @returns {string[]}
 */
export function orphanBaselines(baselinesDir, entries) {
  if (!existsSync(baselinesDir)) return [];
  const captured = new Set(entries.filter((e) => e.screen !== null).map((e) => e.variant));
  const known = new Set(
    entries.filter((e) => e.screen !== null).map((e) => `${e.variant}/${e.screen ?? ""}`),
  );
  /** @type {string[]} */
  const out = [];
  for (const variant of readdirSync(baselinesDir, { withFileTypes: true })) {
    if (!variant.isDirectory() || !captured.has(variant.name)) continue;
    for (const file of readdirSync(path.join(baselinesDir, variant.name))) {
      if (!file.endsWith(".png")) continue;
      const id = `${variant.name}/${file.slice(0, -4)}`;
      if (!known.has(id)) out.push(id);
    }
  }
  return out.sort();
}

/**
 * Escapes text for HTML content and attribute values.
 *
 * @param {string} value
 */
export function escapeHtml(value) {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

/** @param {string} s */
const safe = (s) => s.replace(/[^A-Za-z0-9._-]/g, "_");

/**
 * Copies an entry's images into `<out>/images/<variant>/` and answers the
 * entry with page-relative paths (`null` for an image that does not exist).
 *
 * @param {Entry} entry
 * @param {string} out
 * @returns {Entry}
 */
export function copyImages(entry, out) {
  /**
   * @param {string | null} source
   * @param {string} kind
   */
  const copy = (source, kind) => {
    if (!source || !existsSync(source)) return null;
    const rel = `images/${safe(entry.variant)}/${safe(entry.screen ?? "variant")}-${kind}.png`;
    const target = path.join(out, ...rel.split("/"));
    mkdirSync(path.dirname(target), { recursive: true });
    copyFileSync(source, target);
    return rel;
  };
  return {
    ...entry,
    baseline: copy(entry.baseline, "baseline"),
    actual: copy(entry.actual, "actual"),
    diff: copy(entry.diff, "diff"),
  };
}

/** @type {Partial<Record<string, string>>} */
const STATUS_TEXT = {
  changed: "Changed",
  new: "New (no baseline)",
  error: "Error",
  passed: "Matches baseline",
  captured: "Captured (no comparison)",
  skipped: "Skipped",
};

/**
 * The text shown for a status.
 *
 * @param {string} status
 */
function statusText(status) {
  return STATUS_TEXT[status] ?? status;
}

/**
 * @param {string} label
 * @param {string | null} src
 * @param {string} alt
 * @param {string} missing
 */
function figure(label, src, alt, missing) {
  const body = src
    ? `<a href="${escapeHtml(src)}" target="_blank" rel="noopener"><img src="${escapeHtml(src)}" alt="${escapeHtml(alt)}" loading="lazy"></a>`
    : `<div class="none">${escapeHtml(missing)}</div>`;
  return `<figure><figcaption>${escapeHtml(label)}</figcaption>${body}</figure>`;
}

/** The images of an entry: what exists, plus placeholders where a comparison lacks one. */
/** @param {Entry} e */
function figures(e) {
  const what = `${e.screen ?? ""} · ${e.theme} · ${e.density}`;
  const compared = e.status === "new" || e.status === "changed" || e.status === "passed";
  const out = [];
  if (e.baseline || compared)
    out.push(figure("Baseline", e.baseline, `baseline of ${what}`, "No baseline yet"));
  out.push(figure("Actual", e.actual, `actual ${what}`, "Not captured"));
  if (e.diff || e.status === "changed")
    out.push(figure("Difference", e.diff, `difference of ${what}`, "No difference image"));
  return out.join("");
}

/** @param {Entry} e */
function entryHtml(e) {
  const message = e.message ? `<pre class="message">${escapeHtml(e.message)}</pre>` : "";
  const screen = e.screen ?? "";
  return `<div class="entry" data-status="${e.status}" id="${escapeHtml(`${e.variant}--${screen}`)}">
<div class="head"><span class="badge">${escapeHtml(statusText(e.status))}</span><h3>${escapeHtml(screen)}</h3></div>${message}
<div class="images">${figures(e)}</div>
</div>`;
}

/**
 * Skipped variants (no screens) in one list instead of a section each.
 *
 * @param {Entry[]} skipped
 */
function skippedHtml(skipped) {
  if (skipped.length === 0) return "";
  const rows = skipped
    .map(
      (e) =>
        `<div class="entry" data-status="skipped"><div class="head"><span class="badge">${escapeHtml(statusText("skipped"))}</span><h3>${escapeHtml(`${e.theme} · ${e.density}`)}</h3></div>${e.message ? `<pre class="message">${escapeHtml(e.message)}</pre>` : ""}</div>`,
    )
    .join("\n");
  return `<section class="variant" data-worst="skipped"><h2>Skipped variants</h2>${rows}</section>`;
}

/**
 * The page; `entries` carry page-relative image paths (see {@link copyImages}).
 *
 * @param {Entry[]} entries
 * @param {{ summary: Record<string, number>, orphans?: string[], generatedAt: string,
 *   results: string }} options
 */
export function renderHtml(entries, { summary, orphans = [], generatedAt, results }) {
  /** @type {Map<string, Entry[]>} */
  const groups = new Map();
  for (const e of entries.filter((x) => x.screen !== null)) {
    const list = groups.get(e.variant) ?? [];
    list.push(e);
    groups.set(e.variant, list);
  }
  const chips = STATUSES.map(
    (s) =>
      `<button type="button" class="chip" data-filter="${s}" aria-pressed="false"><span class="badge" data-status="${s}">${escapeHtml(statusText(s))}</span> <b>${String(summary[s] ?? 0)}</b></button>`,
  ).join("");
  const sections = [...groups.entries()]
    .map(([variant, list]) => {
      const theme = list[0]?.theme ?? "";
      const density = list[0]?.density ?? "";
      const title =
        variant === "current"
          ? `Current theme (${theme}, harness self-check)`
          : `${theme} · ${density}`;
      const worst = STATUSES.find((s) => list.some((e) => e.status === s)) ?? "";
      return `<section class="variant" data-worst="${worst}"><h2>${escapeHtml(title)}</h2>${list.map(entryHtml).join("\n")}</section>`;
    })
    .join("\n");
  const skipped = skippedHtml(entries.filter((e) => e.screen === null));
  const orphanHtml =
    orphans.length > 0
      ? `<section class="variant"><h2>Baselines without a screen</h2><p>These baselines belong to captured variants but no screen of this run; delete them if the screen was removed or renamed.</p><ul>${orphans.map((o) => `<li><code>${escapeHtml(o)}</code></li>`).join("")}</ul></section>`
      : "";
  const empty =
    entries.length === 0
      ? `<p class="empty">The manifest is empty: no visual run has written results to <code>${escapeHtml(results)}</code> yet.</p>`
      : "";
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Visual review</title>
<style>
:root { --bg: #f6f7f9; --fg: #1d2330; --muted: #5d6676; --card: #ffffff; --line: #d9dde5;
  --changed: #b42318; --new: #8a5a00; --error: #b42318; --passed: #1f7a4a; --captured: #2c5fb8; --skipped: #5d6676; color-scheme: light; }
@media (prefers-color-scheme: dark) { :root { --bg: #14171d; --fg: #e6e9ef; --muted: #9aa3b2; --card: #1d2129; --line: #323846;
  --changed: #ff8a80; --new: #f2c14e; --error: #ff8a80; --passed: #6fd3a0; --captured: #8db4ff; --skipped: #9aa3b2; color-scheme: dark; } }
* { box-sizing: border-box; }
body { margin: 0; background: var(--bg); color: var(--fg); font: 14px/1.45 "Segoe UI Variable", "Segoe UI", system-ui, sans-serif; }
header, main { max-width: 1600px; margin: 0 auto; padding: 16px; }
header h1 { margin: 0 0 4px; font-size: 22px; }
header p { margin: 0 0 12px; color: var(--muted); }
.filters { display: flex; flex-wrap: wrap; gap: 8px; }
.chip { font: inherit; color: inherit; background: var(--card); border: 1px solid var(--line); border-radius: 999px; padding: 4px 12px; cursor: pointer; }
.chip[aria-pressed="true"] { outline: 2px solid currentColor; }
.badge { font-weight: 600; }
[data-status="changed"] .badge, .badge[data-status="changed"] { color: var(--changed); }
[data-status="new"] .badge, .badge[data-status="new"] { color: var(--new); }
[data-status="error"] .badge, .badge[data-status="error"] { color: var(--error); }
[data-status="passed"] .badge, .badge[data-status="passed"] { color: var(--passed); }
[data-status="captured"] .badge, .badge[data-status="captured"] { color: var(--captured); }
[data-status="skipped"] .badge, .badge[data-status="skipped"] { color: var(--skipped); }
.variant { background: var(--card); border: 1px solid var(--line); border-radius: 10px; padding: 12px 16px; margin: 16px 0; }
.variant h2 { margin: 0 0 8px; font-size: 18px; }
.entry { border-top: 1px solid var(--line); padding: 12px 0; }
.entry:first-of-type { border-top: 0; }
.head { display: flex; gap: 12px; align-items: baseline; }
.head h3 { margin: 0; font-size: 15px; }
.message { white-space: pre-wrap; overflow-wrap: anywhere; color: var(--muted); margin: 6px 0; font-size: 12px; }
.images { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(100%, 320px), 1fr)); gap: 12px; margin-top: 8px; }
figure { margin: 0; min-width: 0; }
figcaption { color: var(--muted); font-size: 12px; margin-bottom: 4px; }
figure img { display: block; width: 100%; height: auto; border: 1px solid var(--line); border-radius: 6px; background: repeating-conic-gradient(#8881 0 25%, transparent 0 50%) 0 0 / 16px 16px; }
.none { display: grid; place-items: center; aspect-ratio: 16 / 10; border: 1px dashed var(--line); border-radius: 6px; color: var(--muted); }
.empty { color: var(--muted); }
body.filtered .entry:not(.shown), body.filtered .variant:not(.shown) { display: none; }
code { overflow-wrap: anywhere; }
</style>
</head>
<body>
<header>
<h1>Visual review</h1>
<p>Generated ${escapeHtml(generatedAt)} from <code>${escapeHtml(results)}</code>. Accept changes with <code>npm run visual:update</code>.</p>
<div class="filters" role="group" aria-label="Filter by status">${chips}</div>
</header>
<main>
${empty}${sections}
${skipped}
${orphanHtml}
</main>
<script>
(() => {
  const active = new Set();
  const chips = document.querySelectorAll(".chip");
  function apply() {
    document.body.classList.toggle("filtered", active.size > 0);
    for (const section of document.querySelectorAll(".variant")) {
      let any = false;
      for (const entry of section.querySelectorAll(".entry")) {
        const shown = active.has(entry.dataset.status);
        entry.classList.toggle("shown", shown);
        any = any || shown;
      }
      section.classList.toggle("shown", any);
    }
  }
  for (const chip of chips) {
    chip.addEventListener("click", () => {
      const s = chip.dataset.filter;
      if (active.has(s)) active.delete(s); else active.add(s);
      chip.setAttribute("aria-pressed", String(active.has(s)));
      apply();
    });
  }
})();
</script>
</body>
</html>
`;
}

/**
 * Builds the page; answers its path and the summary.
 *
 * @param {Options} options
 * @param {Date} [now]
 */
export function buildReview(options, now = new Date()) {
  const entries = readManifest(path.join(options.results, "manifest"));
  rmSync(path.join(options.out, "images"), {
    recursive: true,
    force: true,
    maxRetries: 5,
    retryDelay: 200,
  });
  mkdirSync(options.out, { recursive: true });
  const shown = entries.map((e) => copyImages(e, options.out));
  const summary = summarize(entries);
  const orphans = orphanBaselines(options.baselines, entries);
  const file = path.join(options.out, "index.html");
  writeFileSync(
    file,
    renderHtml(shown, {
      summary,
      orphans,
      generatedAt: now.toISOString(),
      results: options.results,
    }),
  );
  writeFileSync(
    path.join(options.out, "summary.json"),
    `${JSON.stringify({ summary, orphans, entries }, null, 2)}\n`,
  );
  return { file, summary, orphans, entries };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const { file, summary } = buildReview(parseArgs(process.argv.slice(2)));
    const counts = STATUSES.filter((s) => (summary[s] ?? 0) > 0).map(
      (s) => `${String(summary[s])} ${s}`,
    );
    console.log(`visual review: ${counts.join(", ") || "no results"}`);
    console.log(file);
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 2;
  }
}
