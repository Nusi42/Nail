// Text reports of the duplicate check (wave Q2, ADR 0171): the summary of
// `npm run check`, the per-package lists the Q2 packages work from, and the
// Markdown baseline written into the ADR. Tests in
// scripts/test/duplicates.test.mjs.
import { CLONE_CHECKS } from "./index.mjs";
import { PACKAGES } from "./owners.mjs";
import { SSOT_CHECKS } from "./ssot.mjs";

/**
 * @typedef {import("./index.mjs").Classified} Classified
 * @typedef {import("./owners.mjs").Package} Package
 */

/** Every check id in report order. */
export const CHECK_IDS = [...Object.keys(CLONE_CHECKS), ...Object.keys(SSOT_CHECKS)];

/**
 * What a check finds and how to fix it.
 *
 * @param {string} check
 */
export function describeCheck(check) {
  return (
    /** @type {Record<string, string>} */ (CLONE_CHECKS)[check] ??
    /** @type {Record<string, string>} */ (SSOT_CHECKS)[check] ??
    check
  );
}

/**
 * Counts per check and package (allowed findings left out).
 *
 * @param {Classified[]} findings
 * @returns {Map<string, Map<Package, { known: number, new: number }>>}
 */
export function counts(findings) {
  /** @typedef {Map<Package, { known: number, new: number }>} Row */
  /** @type {Map<string, Row>} */
  const table = new Map(CHECK_IDS.map((id) => [id, /** @type {Row} */ (new Map())]));
  for (const f of findings) {
    if (f.status === "allowed") continue;
    const row = table.get(f.finding.check) ?? /** @type {Row} */ (new Map());
    const cell = row.get(f.owner) ?? { known: 0, new: 0 };
    if (f.status === "new") cell.new++;
    else cell.known++;
    row.set(f.owner, cell);
    table.set(f.finding.check, row);
  }
  return table;
}

/**
 * The console summary: one line per check with its counts per package.
 *
 * @param {Classified[]} findings
 */
export function summaryText(findings) {
  const width = Math.max(...CHECK_IDS.map((c) => c.length));
  const lines = [];
  for (const [check, row] of counts(findings)) {
    let total = 0;
    let fresh = 0;
    const parts = [];
    for (const pkg of PACKAGES) {
      const cell = row.get(pkg);
      if (cell === undefined) continue;
      total += cell.known + cell.new;
      fresh += cell.new;
      parts.push(`${pkg.replace(/^WP-/, "")} ${String(cell.known + cell.new)}`);
    }
    const tail = parts.length > 0 ? `  (${parts.join(", ")})` : "";
    lines.push(
      `  ${check.padEnd(width)}  ${String(total).padStart(5)}  new ${String(fresh).padStart(3)}${tail}`,
    );
  }
  return lines.join("\n");
}

/**
 * One line per finding.
 *
 * @param {Classified} f
 */
export function findingLine(f) {
  const status = f.status === "new" ? "NEW " : f.status === "allowed" ? "ok  " : "    ";
  return `${status}[${f.finding.check}] ${f.finding.detail}`;
}

/**
 * The findings of one package (or all), grouped by check, for the package's
 * work list.
 *
 * @param {Classified[]} findings
 * @param {Package | undefined} owner
 */
export function listText(findings, owner) {
  const lines = [];
  for (const check of CHECK_IDS) {
    const mine = findings.filter(
      (f) =>
        f.finding.check === check &&
        f.status !== "allowed" &&
        (owner === undefined || f.owner === owner),
    );
    if (mine.length === 0) continue;
    lines.push(`\n## ${check} (${String(mine.length)}): ${describeCheck(check)}`);
    for (const f of mine)
      lines.push(`${findingLine(f)}${owner === undefined ? `  -> ${f.owner}` : ""}`);
  }
  return lines.join("\n");
}

/**
 * The largest clone clusters: clones grouped by their pair of files, ranked
 * by duplicated lines.
 *
 * @param {Classified[]} findings
 * @param {number} top
 * @returns {{ pair: string, clones: number, lines: number, owner: Package }[]}
 */
export function cloneClusters(findings, top) {
  /** @type {Map<string, { pair: string, clones: number, lines: number, owner: Package }>} */
  const groups = new Map();
  for (const f of findings) {
    if (f.finding.check !== "clone" && f.finding.check !== "css-clone") continue;
    const pair = f.finding.files.join(" <> ");
    const lines = Number(/\((\d+) lines/.exec(f.finding.detail)?.[1] ?? "0");
    const g = groups.get(pair) ?? { pair, clones: 0, lines: 0, owner: f.owner };
    g.clones++;
    g.lines += lines;
    groups.set(pair, g);
  }
  return [...groups.values()]
    .sort((a, b) => b.lines - a.lines || a.pair.localeCompare(b.pair))
    .slice(0, top);
}

/**
 * Counts per area (first two path segments; `ui/src/<dir>`) for the baseline.
 *
 * @param {Classified[]} findings
 * @returns {[string, number][]}
 */
export function areaCounts(findings) {
  /** @type {Map<string, number>} */
  const map = new Map();
  for (const f of findings) {
    if (f.status === "allowed") continue;
    const areas = new Set(
      f.finding.files.map((file) => {
        const p = file.split("/");
        return p[0] === "ui"
          ? p.slice(0, 3).join("/")
          : p.slice(0, Math.min(2, p.length - 1) || 1).join("/");
      }),
    );
    const key = [...areas].sort().join(" + ");
    map.set(key, (map.get(key) ?? 0) + 1);
  }
  return [...map].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
}

/**
 * The Markdown baseline (ADR / plan): totals per check and package, areas,
 * top clone clusters.
 *
 * @param {Classified[]} findings
 * @param {{ files: number, lines: number, duplicatedLines: number }} stats
 */
export function markdownBaseline(findings, stats) {
  const out = [];
  out.push(
    `Scanned ${String(stats.files)} files, ${String(stats.lines)} lines; jscpd counts ${String(stats.duplicatedLines)} duplicated lines.`,
    "",
    `| Check | ${PACKAGES.join(" | ")} | Total |`,
    `| --- | ${PACKAGES.map(() => "---:").join(" | ")} | ---: |`,
  );
  for (const [check, row] of counts(findings)) {
    const cells = PACKAGES.map((p) => {
      const c = row.get(p);
      return c === undefined ? 0 : c.known + c.new;
    });
    out.push(`| ${check} | ${cells.join(" | ")} | ${String(cells.reduce((a, b) => a + b, 0))} |`);
  }
  out.push("", "| Area | Findings |", "| --- | ---: |");
  for (const [area, n] of areaCounts(findings).slice(0, 30)) out.push(`| ${area} | ${String(n)} |`);
  out.push(
    "",
    "| Clone cluster (file pair) | Clones | Lines | Owner |",
    "| --- | ---: | ---: | --- |",
  );
  for (const c of cloneClusters(findings, 25)) {
    out.push(`| ${c.pair} | ${String(c.clones)} | ${String(c.lines)} | ${c.owner} |`);
  }
  return out.join("\n");
}
