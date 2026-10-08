// Arguments and verdict of scripts/check-duplicates.mjs (wave Q2, ADR 0171).
// Tests in scripts/test/duplicates.test.mjs.
import { PACKAGES } from "./owners.mjs";

/**
 * @typedef {import("./owners.mjs").Package} Package
 * @typedef {import("./index.mjs").Classified} Classified
 *
 * @typedef {object} Options
 * @property {boolean} strict fail on every finding that is not an exception (known ones too)
 * @property {Package | undefined} owner only this package's findings
 * @property {boolean} list print the findings
 * @property {boolean} updateBaseline rewrite the baselines (the integrator)
 * @property {string | undefined} markdown write the Markdown baseline here
 * @property {string | undefined} json write every classified finding here
 * @property {string[]} files only new findings touching these files fail (the git hooks)
 */

export const USAGE = `node scripts/check-duplicates.mjs [--strict] [--owner WP-…] [--list]
    [--update-baseline] [--markdown FILE] [--json FILE] [--files FILE…]

  (default)          new findings (not in scripts/duplicates/*baseline.json) fail
  --strict           every finding fails (the Q2 gate; with --owner: one package's)
  --owner WP-…       only that package's findings (${PACKAGES.join(", ")})
  --list             print the findings, not only the new ones
  --update-baseline  rewrite the baselines from this run (integrator only)
  --files FILE…      only new findings that touch these files fail (git hooks)`;

/**
 * @param {string[]} argv
 * @returns {Options}
 */
export function parseArgs(argv) {
  /** @type {Options} */
  const options = {
    strict: false,
    owner: undefined,
    list: false,
    updateBaseline: false,
    markdown: undefined,
    json: undefined,
    files: [],
  };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i] ?? "";
    const value = () => {
      const next = argv[++i];
      if (next === undefined) throw new Error(`${arg} needs a value`);
      return next;
    };
    if (arg === "--strict") options.strict = true;
    else if (arg === "--list") options.list = true;
    else if (arg === "--update-baseline") options.updateBaseline = true;
    else if (arg === "--markdown") options.markdown = value();
    else if (arg === "--json") options.json = value();
    else if (arg === "--owner") {
      const owner = value();
      const known = PACKAGES.find((p) => p === owner);
      if (known === undefined)
        throw new Error(`unknown package ${owner} (known: ${PACKAGES.join(", ")})`);
      options.owner = known;
    } else if (arg === "--files") {
      options.files.push(...argv.slice(i + 1).map((f) => f.split("\\").join("/")));
      break;
    } else throw new Error(`unknown argument ${arg}\n\n${USAGE}`);
  }
  return options;
}

/**
 * The findings that make the run fail.
 *
 * @param {Classified[]} findings
 * @param {Options} options
 * @returns {Classified[]}
 */
export function failing(findings, options) {
  const touched = new Set(options.files);
  const touches = (/** @type {Classified} */ f) =>
    f.finding.files.some((file) => touched.has(file));
  // A new copy of a known fragment may be reported on another pair of files
  // (jscpd counts copies per fragment): it fails when any copy is touched.
  const groups = new Set(
    findings.flatMap((f) => (f.finding.group !== undefined && touches(f) ? [f.finding.group] : [])),
  );
  return findings.filter((f) => {
    if (f.status === "allowed") return false;
    if (options.owner !== undefined && f.owner !== options.owner) return false;
    if (options.strict) return true;
    if (f.status !== "new") return false;
    if (touched.size === 0 || touches(f)) return true;
    return f.finding.group !== undefined && groups.has(f.finding.group);
  });
}
