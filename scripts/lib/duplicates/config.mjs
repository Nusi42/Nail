// The settings of the duplicate check (wave Q2, SSOT / ODR / DRY): one place
// for the detector's thresholds, the scanned formats and every ignore, with
// the reason for each (ADR 0171). scripts/check-duplicates.mjs runs it;
// tests in scripts/test/duplicates.test.mjs.

/**
 * jscpd's token-based clone detection (jscpd 5, Rust engine, MIT). 50 tokens
 * and 5 lines are jscpd's own defaults; calibrated in ADR 0171 against the
 * whole repository: a sample of the smallest clones (50–70 tokens) showed
 * copied helpers, mock-server methods, error conversions and test flows —
 * real duplicates — and almost no idioms. `mild` skips whitespace only, so a
 * copied block with its comments still counts as one clone.
 */
export const JSCPD = {
  minTokens: 50,
  minLines: 5,
  mode: "mild",
  /** Source languages (Fluent, KDL and JSON have their own checks or none: ADR 0171). */
  formats: ["rust", "typescript", "tsx", "javascript", "css"],
};

/**
 * Paths jscpd never scans, each with its reason. Everything else that git
 * tracks is scanned, tests included (copied test flows become helpers).
 * `.gitignore`d paths (node_modules, target, dist, .nail-build) are skipped
 * by jscpd itself.
 *
 * @type {{ glob: string, reason: string }[]}
 */
export const IGNORES = [
  {
    glob: "**/generated/**",
    reason:
      "ts-rs output (ui/src/api/generated, packages/plugin-api/src/generated): generated from nail-api, the single source",
  },
  {
    glob: "crates/nail-store/libsqlite3-sys-sqlite3mc/bindings/**",
    reason: "bindgen output of the SQLite3 Multiple Ciphers header (vendored, ADR 0095)",
  },
  {
    glob: "**/fixtures/**",
    reason: "test fixtures: recorded server answers, sample profiles and files (data, not code)",
  },
  { glob: "**/golden/**", reason: "golden files: expected outputs compared byte for byte" },
  { glob: "**/snapshots/**", reason: "snapshot files: expected outputs compared byte for byte" },
  {
    glob: "**/corpus/**",
    reason: "test corpora (.eml, .ics, .vcf, crypto vectors): sample data from other programs",
  },
  {
    glob: "e2e/visual/baselines/**",
    reason: "visual regression baselines (images)",
  },
  {
    glob: "**/node_modules/**",
    reason: "installed packages (also gitignored)",
  },
  { glob: "**/target/**", reason: "Cargo build output (also gitignored)" },
  { glob: "**/dist/**", reason: "build output (also gitignored)" },
  { glob: "**/.nail-build/**", reason: "build and test output (also gitignored)" },
];

/**
 * The check's own code and tests: they describe mirrors, so the
 * `mirror-comment` check skips them.
 */
export const SELF =
  /^scripts\/(check-duplicates\.mjs|lib\/duplicates\/|test\/duplicates\.test\.mjs)/;

/** Where the check keeps its data (baselines, permanent exceptions). */
export const DATA_DIR = "scripts/duplicates";

/**
 * Files of {@link DATA_DIR}: the reasoned exceptions of the SSOT checks, the
 * hand-mirror inventory and the optional ratchet baselines (recorded findings
 * that do not fail; Q2 used them during the wave and deleted them at the
 * integration, ADR 0176, so every finding fails while they are absent).
 */
export const DATA_FILES = {
  /** jscpd's own baseline format (`--baseline`): fingerprints of known clones. */
  jscpdBaseline: "jscpd-baseline.json",
  /** The same for the CSS-in-TS scan. */
  cssBaseline: "css-baseline.json",
  /** Keys of known SSOT findings, per check. */
  ssotBaseline: "ssot-baseline.json",
  /** Findings that are not violations, per check, each with its reason. */
  allow: "allow.json",
  /** Known hand mirrors no detector finds (the `mirror-inventory` check). */
  mirrors: "mirrors.json",
};

/**
 * Numbers too common to be one fact (HTTP statuses, unit factors, byte and
 * percent limits) for the repeated-magic-number check.
 */
export const COMMON_NUMBERS = new Set([
  100, 200, 201, 204, 255, 256, 400, 401, 403, 404, 500, 502, 503, 999, 1000, 1024, 3600, 60_000,
  65_535, 65_536, 86_400, 1_000_000,
]);

/** A number is "repeated" when it appears in at least this many non-test source files. */
export const MAGIC_NUMBER_MIN_FILES = 3;

/** Lowest number the magic-number check looks at. */
export const MAGIC_NUMBER_MIN = 100;
