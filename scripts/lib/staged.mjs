// The pre-commit checks (quality baseline Q1, ADR 0123; the duplicate check
// of wave Q2, ADR 0171): which fast checks run on which staged files. scripts/pre-commit.mjs runs them;
// scripts/test/staged.test.mjs tests this module.

/**
 * @typedef {object} HookCheck
 * @property {string} id
 * @property {string} tool executable, or "node" with a `<bin:pkg>` script
 * @property {string[]} args arguments before the file list
 * @property {string[]} files the staged files this check reads
 * @property {string} install
 */

const RUST = /\.rs$/;
const TOML = /\.toml$/;
const MARKDOWN = /\.md$/;
const SCRIPT = /\.(ts|tsx|mts|cts|js|mjs|cjs)$/;
/**
 * Files the duplicate check reads (code, Fluent catalogs, the default KDL)
 * and its own data (scripts/duplicates/).
 */
const DUPLICATES = /\.(rs|ts|tsx|mts|cts|js|mjs|cjs|ftl|kdl)$|^scripts\/duplicates\//;
/** Extensions Prettier formats in this repository (it skips ignored files itself). */
const PRETTIER = /\.(ts|tsx|mts|cts|js|mjs|cjs|json|jsonc|ya?ml|css|html)$/;

/** The git arguments that list the staged files (added, copied, modified, renamed). */
export const STAGED_FILES_ARGS = ["diff", "--cached", "--name-only", "--diff-filter=ACMR", "-z"];

/**
 * Parses `git diff --cached --name-only -z` output.
 *
 * @param {string} output
 */
export function parseStaged(output) {
  return output.split("\0").filter(Boolean);
}

/**
 * The checks for a set of staged files (only those with files to check).
 *
 * @param {string[]} files staged paths (repository-relative, `/` separators)
 * @returns {HookCheck[]}
 */
export function hookChecks(files) {
  /** @param {RegExp} pattern */
  const pick = (pattern) => files.filter((f) => pattern.test(f));
  /** @type {HookCheck[]} */
  const checks = [
    {
      id: "rustfmt",
      tool: "rustfmt",
      args: ["--check", "--edition", "2024"],
      files: pick(RUST),
      install: "rustup component add rustfmt",
    },
    {
      id: "prettier",
      tool: "node",
      args: ["<bin:prettier>", "--check", "--ignore-unknown"],
      files: pick(PRETTIER),
      install: "npm ci",
    },
    {
      id: "eslint",
      tool: "node",
      args: ["<bin:eslint>", "--max-warnings", "0", "--no-warn-ignored"],
      files: pick(SCRIPT),
      install: "npm ci",
    },
    {
      id: "taplo-fmt",
      tool: "taplo",
      args: ["fmt", "--check"],
      files: pick(TOML),
      install: "cargo install taplo-cli --locked",
    },
    {
      id: "taplo-lint",
      tool: "taplo",
      args: ["check", "--no-schema"],
      files: pick(TOML),
      install: "cargo install taplo-cli --locked",
    },
    {
      id: "markdownlint",
      tool: "node",
      args: ["<bin:markdownlint-cli2>"],
      files: pick(MARKDOWN),
      install: "npm ci",
    },
    {
      id: "typos",
      tool: "typos",
      args: ["--force-exclude"],
      files,
      install: "pip install -r requirements-dev.txt",
    },
    {
      id: "editorconfig",
      tool: "node",
      args: ["scripts/check-editorconfig.mjs"],
      files,
      install: "npm ci",
    },
    {
      // Scans the whole checkout (about 3 s); fails on new copies and new
      // SSOT findings that touch these files.
      id: "duplicates",
      tool: "node",
      args: ["scripts/check-duplicates.mjs", "--files"],
      files: pick(DUPLICATES),
      install: "npm ci",
    },
  ];
  return checks.filter((c) => c.files.length > 0);
}

/**
 * Splits a long file list into command lines that stay below Windows'
 * 32k-character limit.
 *
 * @param {string[]} files
 * @param {number} [maxChars]
 * @returns {string[][]}
 */
export function chunkFiles(files, maxChars = 24000) {
  /** @type {string[][]} */
  const chunks = [];
  /** @type {string[]} */
  let current = [];
  let length = 0;
  for (const file of files) {
    if (current.length > 0 && length + file.length + 1 > maxChars) {
      chunks.push(current);
      current = [];
      length = 0;
    }
    current.push(file);
    length += file.length + 1;
  }
  if (current.length > 0) chunks.push(current);
  return chunks;
}
