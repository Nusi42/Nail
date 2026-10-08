// The duplicate check of wave Q2 (SSOT / ODR / DRY, ADR 0171): jscpd over the
// code, jscpd over the CSS-in-TS, and the SSOT checks; every finding gets an
// owner (a Q2 package) and a status against the ratchet baselines and the
// reasoned exceptions. scripts/check-duplicates.mjs is the command line;
// tests in scripts/test/duplicates.test.mjs.
import fs from "node:fs";
import path from "node:path";

import { readJson } from "../json.mjs";
import { DATA_DIR, DATA_FILES } from "./config.mjs";
import { clonePlace, runJscpd } from "./jscpd.mjs";
import { ownerOf } from "./owners.mjs";
import { CODE, readSources } from "./sources.mjs";
import {
  baselineIdentity,
  cssBlocks,
  cssLengths,
  fluentDuplicates,
  magicNumbers,
  maskedCss,
  mirrorComments,
  mirrorInventory,
  rustTsConstants,
  rustTsLists,
  sameConstants,
  SSOT_CHECKS,
  themeLiterals,
} from "./ssot.mjs";

/**
 * @typedef {import("./ssot.mjs").Finding} Finding
 * @typedef {import("./owners.mjs").Package} Package
 *
 * @typedef {object} Classified
 * @property {Finding} finding
 * @property {Package} owner
 * @property {"new" | "known" | "allowed"} status `known` = in the baseline
 *
 * @typedef {Record<string, Record<string, { reason: string, pinnedBy?: string }>>} Allow
 * @typedef {{ version: 1, findings: Record<string, string[]> }} SsotBaseline
 *
 * @typedef {object} Result
 * @property {Classified[]} findings
 * @property {string[]} problems broken exceptions (stale, or a missing pinning test)
 * @property {{ files: number, lines: number, duplicatedLines: number }} stats jscpd's totals for the code
 */

/** The clone checks (jscpd), next to the SSOT checks of ssot.mjs. */
export const CLONE_CHECKS = {
  clone:
    "copied code (jscpd, ≥ 50 tokens and ≥ 5 lines): extract a shared function, component or test helper",
  "css-clone":
    "copied CSS rules in CSS-in-TS (jscpd's CSS tokenizer on the template literals): share the rule",
};

/**
 * @param {string} root
 * @param {string} name a {@link DATA_FILES} value
 */
export function dataFile(root, name) {
  return path.join(root, DATA_DIR, name);
}

/**
 * @param {string} file
 * @returns {Allow}
 */
export function readAllow(file) {
  return fs.existsSync(file) ? /** @type {Allow} */ (readJson(file)) : {};
}

/**
 * @param {string} file
 * @returns {SsotBaseline}
 */
export function readSsotBaseline(file) {
  return fs.existsSync(file)
    ? /** @type {SsotBaseline} */ (readJson(file))
    : { version: 1, findings: {} };
}

/**
 * Converts jscpd clones to findings.
 *
 * @param {import("./jscpd.mjs").Report} report
 * @param {string} check
 * @param {{ mapName?: (name: string) => string, baseline: boolean }} options
 *   `baseline: false` = no baseline was given: every clone is new
 * @returns {(Finding & { isNew: boolean })[]}
 */
export function cloneFindings(report, check, options) {
  return report.duplicates.map((d) => {
    const a = clonePlace(d.firstFile, options.mapName);
    const b = clonePlace(d.secondFile, options.mapName);
    const files = [...new Set([a, b].map((p) => p.replace(/:\d+-\d+$/, "")))].sort();
    return {
      check,
      key: `${a} <> ${b}`,
      files,
      detail: `${a} <> ${b} (${String(d.lines)} lines, ${String(d.tokens)} tokens)`,
      group: d.fragment.replace(/\s+/g, " ").trim(),
      isNew: !options.baseline || d.isNew === true,
    };
  });
}

/**
 * Classifies SSOT findings against the exceptions and the baseline, and
 * checks the exceptions themselves.
 *
 * @param {Finding[]} findings
 * @param {Allow} allow
 * @param {SsotBaseline} baseline
 * @param {(file: string) => string | undefined} read a repository file's text, if it exists
 * @param {string[]} checks the checks that ran (exceptions of other checks are not judged)
 * @returns {{ classified: Classified[], problems: string[] }}
 */
export function classifySsot(findings, allow, baseline, read, checks) {
  /** @type {string[]} */
  const problems = [];
  const keys = new Set(findings.map((f) => `${f.check}\0${f.key}`));
  for (const check of checks) {
    for (const [key, entry] of Object.entries(allow[check] ?? {})) {
      if (!keys.has(`${check}\0${key}`)) {
        problems.push(`allow.json: ${check} "${key}" matches nothing any more; remove it`);
      }
      if (entry.pinnedBy !== undefined) {
        const text = read(entry.pinnedBy);
        if (text === undefined)
          problems.push(`allow.json: ${check} "${key}": pinning test ${entry.pinnedBy} is missing`);
        else if (!text.includes(key)) {
          problems.push(
            `allow.json: ${check} "${key}": pinning test ${entry.pinnedBy} does not mention it`,
          );
        }
      }
    }
  }
  const classified = findings.map((finding) => {
    const allowed = allow[finding.check]?.[finding.key] !== undefined;
    const known = baseline.findings[finding.check]?.includes(baselineIdentity(finding)) ?? false;
    /** @type {Classified} */
    const c = {
      finding,
      owner: ownerOf(finding.files, { mirror: finding.mirror ?? false }),
      status: allowed ? "allowed" : known ? "known" : "new",
    };
    return c;
  });
  return { classified, problems };
}

/**
 * Runs every check over the checkout. Each run works in its own scratch
 * directory below `<buildDir>/duplicates/` (the post-edit hook may run
 * several at once) and removes it afterwards.
 *
 * @param {string} root
 * @param {{ buildDir: string, updateBaseline?: boolean }} options
 * @returns {Result}
 */
export function collect(root, options) {
  const out = path.join(
    options.buildDir,
    "duplicates",
    `run-${String(process.pid)}-${String(Date.now())}`,
  );
  try {
    return collectIn(root, out, options);
  } finally {
    fs.rmSync(out, { recursive: true, force: true });
  }
}

/**
 * @param {string} root
 * @param {string} out the run's scratch directory
 * @param {{ updateBaseline?: boolean }} options
 * @returns {Result}
 */
function collectIn(root, out, options) {
  const update = options.updateBaseline === true;
  const code = readSources(root, CODE);
  /** @param {string} name */
  const baselineArg = (name) => {
    const file = dataFile(root, name);
    return update || fs.existsSync(file) ? file : undefined;
  };

  // 1. Copied code.
  const codeBaseline = baselineArg(DATA_FILES.jscpdBaseline);
  const report = runJscpd(root, root, {
    paths: ["."],
    out: path.join(out, "jscpd"),
    baseline: codeBaseline,
    updateBaseline: update,
  });
  const clones = cloneFindings(report, "clone", { baseline: codeBaseline !== undefined });

  // 2. Copied CSS rules in CSS-in-TS: masked copies with the source's line
  // numbers, scanned with jscpd's CSS tokenizer.
  const cssDir = path.join(out, "css-in-ts");
  fs.rmSync(cssDir, { recursive: true, force: true });
  for (const [file, text] of maskedCss(code)) {
    const target = path.join(cssDir, `${file}.css`);
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.writeFileSync(target, text);
  }
  /** @type {(Finding & { isNew: boolean })[]} */
  let cssClones = [];
  if (fs.existsSync(cssDir)) {
    const cssBaseline = baselineArg(DATA_FILES.cssBaseline);
    const cssReport = runJscpd(root, cssDir, {
      paths: ["."],
      out: path.join(out, "jscpd-css"),
      formats: ["css"],
      ignore: false,
      baseline: cssBaseline,
      updateBaseline: update,
    });
    cssClones = cloneFindings(cssReport, "css-clone", {
      baseline: cssBaseline !== undefined,
      mapName: (n) => n.replace(/^\.\//, "").replace(/\.css$/, ""),
    });
  }

  // 3. The SSOT checks.
  const catalogs = readSources(root, /^locales\/en\/[^/]+\.ftl$/);
  const kdl = readSources(root, /^defaults\/(theme|layout)\.kdl$/);
  const blocks = cssBlocks(code);
  /** @param {string} file */
  const read = (file) => {
    const full = path.join(root, file);
    return fs.existsSync(full) ? fs.readFileSync(full, "utf8") : undefined;
  };
  const exists = (/** @type {string} */ f) => read(f) !== undefined;
  const mirrorsFile = dataFile(root, DATA_FILES.mirrors);
  const inventory = mirrorInventory(
    fs.existsSync(mirrorsFile)
      ? /** @type {import("./ssot.mjs").InventoryEntry[]} */ (readJson(mirrorsFile))
      : [],
    exists,
  );
  const ssot = [
    ...mirrorComments(code, exists),
    ...inventory.findings,
    ...rustTsLists(code),
    ...rustTsConstants(code),
    ...sameConstants(code),
    ...fluentDuplicates(catalogs),
    ...themeLiterals(blocks),
    ...cssLengths(blocks, kdl),
    ...magicNumbers(code),
  ];
  const allow = readAllow(dataFile(root, DATA_FILES.allow));
  const ssotBaselineFile = dataFile(root, DATA_FILES.ssotBaseline);
  if (update) {
    /** @type {SsotBaseline} */
    const fresh = { version: 1, findings: {} };
    for (const f of ssot) {
      if (allow[f.check]?.[f.key] !== undefined) continue;
      (fresh.findings[f.check] ??= []).push(baselineIdentity(f));
    }
    for (const list of Object.values(fresh.findings)) list.sort();
    fs.mkdirSync(path.dirname(ssotBaselineFile), { recursive: true });
    fs.writeFileSync(ssotBaselineFile, `${JSON.stringify(fresh, null, 2)}\n`);
  }
  const { classified, problems } = classifySsot(
    ssot,
    allow,
    readSsotBaseline(ssotBaselineFile),
    read,
    Object.keys(SSOT_CHECKS),
  );

  /** @type {Classified[]} */
  const cloneResults = [...clones, ...cssClones].map((finding) => ({
    finding,
    owner: ownerOf(finding.files),
    status: finding.isNew && !update ? "new" : "known",
  }));
  return {
    findings: [...cloneResults, ...classified],
    problems: [...inventory.problems, ...problems],
    stats: {
      files: report.statistics.total.sources,
      lines: report.statistics.total.lines,
      duplicatedLines: report.statistics.total.duplicatedLines,
    },
  };
}
