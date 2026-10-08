// Which Q2 package owns a finding (plan section "Wave Q2", ADR 0171): every
// finding belongs to exactly one package, so the four packages work in
// disjoint directories. Tests in scripts/test/duplicates.test.mjs.

/** The packages of wave Q2. */
export const PACKAGES = /** @type {const} */ ([
  "WP-rust-core",
  "WP-rust-engines",
  "WP-ui",
  "WP-mirrors-tooling",
]);

/** @typedef {(typeof PACKAGES)[number]} Package */

/** Crates of WP-rust-core (every other crate is WP-rust-engines'). */
export const CORE_CRATES = new Set([
  "nail-api",
  "nail-core",
  "nail-store",
  "nail-config",
  "nail-mcp",
  "nail-server",
  "nail-cli",
  "nail-search",
  "nail-rules",
  "nail-triage",
  "nail-sieve",
  "nail-compose",
  "nail-mime",
  "nail-vault",
  "nail-remote-auth",
  "nail-proxy",
  "nail-plugins",
  "nail-portability",
  "nail-bench",
  "nail-util",
]);

/** Fluent catalogs written by Rust code (the rest belong to the UI). */
const CORE_CATALOG = /^locales\/[^/]+\/(core(-[^/]+)?|itip)\.ftl$/;

/**
 * The package that owns a repository path (`/` separators).
 *
 * @param {string} file
 * @returns {Package}
 */
export function ownerOfPath(file) {
  const parts = file.split("/");
  const top = parts[0] ?? "";
  if (top === "crates") return CORE_CRATES.has(parts[1] ?? "") ? "WP-rust-core" : "WP-rust-engines";
  if (top === "app") return "WP-rust-engines";
  if (top === "ui" || top === "defaults") return "WP-ui";
  if (top === "locales") return CORE_CATALOG.test(file) ? "WP-rust-core" : "WP-ui";
  return "WP-mirrors-tooling";
}

/**
 * The one package that owns a finding spanning `files`.
 *
 * - A Rust <-> TypeScript mirror (`mirror`) is WP-mirrors-tooling's, wherever
 *   its files are (it owns every mirror's TypeScript side and generator).
 * - Files of one package: that package.
 * - Rust and TypeScript files, or any file of WP-mirrors-tooling's
 *   directories (scripts, e2e, packages, examples): WP-mirrors-tooling.
 * - WP-rust-core and WP-rust-engines files: WP-rust-engines, which owns the
 *   shared crates `nail-net` and the engine side; it replaces both copies
 *   with one shared function (a cross-package clone is never split).
 *
 * @param {string[]} files
 * @param {{ mirror?: boolean }} [options]
 * @returns {Package}
 */
export function ownerOf(files, options = {}) {
  if (options.mirror === true) return "WP-mirrors-tooling";
  const owners = new Set(files.map(ownerOfPath));
  const [only] = owners;
  if (owners.size === 1 && only !== undefined) return only;
  if (owners.has("WP-mirrors-tooling")) return "WP-mirrors-tooling";
  const rust = owners.has("WP-rust-core") || owners.has("WP-rust-engines");
  if (rust && owners.has("WP-ui")) {
    // Fluent catalogs are text, not code: a UI text that repeats a core
    // text is the UI's to drop.
    if (files.every((f) => f.startsWith("locales/"))) return "WP-ui";
    return "WP-mirrors-tooling";
  }
  return "WP-rust-engines";
}
