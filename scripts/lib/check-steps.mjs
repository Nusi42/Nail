// The steps of `npm run check` (quality baseline Q1, ADR 0123): every
// formatter check and linter of the repository, with zero warnings allowed,
// and the duplicate / SSOT check of wave Q2 (ADR 0171).
// scripts/check.mjs runs them; scripts/test/check-steps.test.mjs tests this
// module.
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { forEachArg } from "./args.mjs";

/**
 * @typedef {object} Step
 * @property {string} id short name for --only / --skip
 * @property {string} title what the step checks
 * @property {string} tool executable (resolved by `resolveTool`)
 * @property {string[]} args `<bin:pkg>` = that npm package's CLI script,
 *   `<npm>` = npm's CLI script (both run with node; see `resolveArgs`)
 * @property {Record<string, string>} [env] extra environment
 * @property {string} install how to install the tool when it is missing
 */

/** How the tools outside npm are installed (docs/dev-setup.md). */
export const INSTALL = {
  cargo: "rustup (https://rustup.rs) with `rustup component add rustfmt clippy`",
  taplo: "cargo install taplo-cli --locked",
  machete: "cargo install cargo-machete --locked",
  deny: "cargo install cargo-deny --locked",
  python: "pip install -r requirements-dev.txt",
  npm: "npm ci",
};

/**
 * The check steps in their run order (fast formatters first).
 *
 * @param {{ ci?: boolean, advisories?: boolean, jobs?: string }} [options]
 *   `advisories`: also run `cargo deny check advisories` (needs the RustSec
 *   database from the network; on by default when the environment sets CI).
 * @returns {Step[]}
 */
export function checkSteps(options = {}) {
  const jobs = options.jobs ?? "4";
  const advisories = options.advisories ?? options.ci ?? false;
  return [
    {
      id: "rustfmt",
      title: "Rust formatting (rustfmt)",
      tool: "cargo",
      args: ["fmt", "--all", "--check"],
      install: INSTALL.cargo,
    },
    {
      id: "rustfmt-interop",
      title: "Rust formatting of scripts/interop (own workspace)",
      tool: "cargo",
      args: ["fmt", "--manifest-path", "scripts/interop/Cargo.toml", "--check"],
      install: INSTALL.cargo,
    },
    {
      id: "taplo-fmt",
      title: "TOML formatting (taplo)",
      tool: "taplo",
      args: ["fmt", "--check"],
      install: INSTALL.taplo,
    },
    {
      id: "taplo-lint",
      title: "TOML syntax (taplo)",
      tool: "taplo",
      args: ["check", "--no-schema"],
      install: INSTALL.taplo,
    },
    {
      id: "prettier",
      title: "Prettier (TS, JS, JSON, YAML, CSS, HTML)",
      tool: "node",
      args: ["<bin:prettier>", "--check", "."],
      install: INSTALL.npm,
    },
    {
      id: "editorconfig",
      title: "EditorConfig (charset, line endings, final newline, whitespace)",
      tool: "node",
      args: ["scripts/check-editorconfig.mjs"],
      install: INSTALL.npm,
    },
    {
      // ADR 0192: patterns from %USERPROFILE%/.config/nail/private-patterns.txt
      // (outside the repository); passes when the file is absent (other machines).
      id: "private",
      title: "No private data (the owner's patterns, every tracked file)",
      tool: "node",
      args: ["scripts/check-private.mjs"],
      install: INSTALL.npm,
    },
    {
      // ADR 0200: no session data in the project notes.
      id: "notes",
      title: "No session data in the project notes",
      tool: "node",
      args: ["scripts/check-notes.mjs"],
      install: INSTALL.npm,
    },
    {
      id: "typos",
      title: "Spelling (typos)",
      tool: "typos",
      args: [],
      install: INSTALL.python,
    },
    {
      id: "markdownlint",
      title: "Markdown (markdownlint-cli2)",
      tool: "node",
      args: ["<bin:markdownlint-cli2>", "**/*.md"],
      install: INSTALL.npm,
    },
    {
      id: "eslint",
      title: "ESLint, type-aware, --max-warnings 0 (all TS / JS)",
      tool: "node",
      args: ["<bin:eslint>", "--max-warnings", "0", "."],
      install: INSTALL.npm,
    },
    {
      id: "tsc",
      title: "TypeScript (every tsconfig)",
      tool: "node",
      args: ["<npm>", "run", "lint:types"],
      install: INSTALL.npm,
    },
    {
      id: "duplicates",
      title: "Duplicates and SSOT (jscpd, mirrors, constants, Fluent texts; every finding fails)",
      tool: "node",
      args: ["scripts/check-duplicates.mjs", "--strict"],
      install: INSTALL.npm,
    },
    {
      id: "clippy",
      title: "clippy -D warnings (workspace, all targets)",
      tool: "cargo",
      args: ["clippy", "-j", jobs, "--workspace", "--all-targets", "--", "-D", "warnings"],
      install: INSTALL.cargo,
    },
    {
      id: "rustdoc",
      title: "rustdoc -D warnings (workspace, no deps)",
      tool: "cargo",
      args: ["doc", "-j", jobs, "--workspace", "--no-deps"],
      env: { RUSTDOCFLAGS: "-D warnings" },
      install: INSTALL.cargo,
    },
    {
      id: "machete",
      title: "Unused Rust dependencies (cargo machete)",
      tool: "cargo-machete",
      args: [],
      install: INSTALL.machete,
    },
    {
      id: "deny",
      title: advisories
        ? "cargo deny (licences, bans, sources, advisories)"
        : "cargo deny (licences, bans, sources; advisories need --advisories or CI=1)",
      tool: "cargo-deny",
      args: advisories
        ? ["--locked", "check"]
        : ["--locked", "check", "licenses", "bans", "sources"],
      install: INSTALL.deny,
    },
  ];
}

/**
 * Selects steps by id (`only` wins over `skip`); unknown ids are an error.
 *
 * @param {Step[]} steps
 * @param {{ only?: string[], skip?: string[] }} selection
 */
export function selectSteps(steps, { only = [], skip = [] }) {
  const ids = new Set(steps.map((s) => s.id));
  const unknown = [...only, ...skip].filter((id) => !ids.has(id));
  if (unknown.length > 0) {
    throw new Error(`unknown check step(s): ${unknown.join(", ")} (known: ${[...ids].join(", ")})`);
  }
  if (only.length > 0) return steps.filter((s) => only.includes(s.id));
  return steps.filter((s) => !skip.includes(s.id));
}

/**
 * Parses `check.mjs` arguments.
 *
 * @param {string[]} argv
 */
export function parseCheckArgs(argv) {
  /** @type {{ only: string[], skip: string[], advisories: boolean, list: boolean, keepGoing: boolean }} */
  const options = { only: [], skip: [], advisories: false, list: false, keepGoing: true };
  forEachArg(argv, (arg, reader) => {
    const value = () => reader.value().split(",").filter(Boolean);
    if (arg === "--only") options.only.push(...value());
    else if (arg === "--skip") options.skip.push(...value());
    else if (arg === "--advisories") options.advisories = true;
    else if (arg === "--list") options.list = true;
    else if (arg === "--fail-fast") options.keepGoing = false;
    else throw new Error(`unknown argument ${arg}`);
  });
  return options;
}

/**
 * Replaces the `<bin:pkg>` / `<npm>` placeholders of step arguments with
 * script paths, so npm tools run through node without a shell (npm / npx
 * are .cmd shims on Windows).
 *
 * @param {string[]} args
 * @param {{ root: string, npmCli?: string, readJson?: (file: string) => unknown }} env
 */
export function resolveArgs(args, env) {
  const readJson =
    env.readJson ??
    ((/** @type {string} */ file) =>
      /** @type {unknown} */ (JSON.parse(fs.readFileSync(file, "utf8"))));
  return args.map((arg) => {
    if (arg === "<npm>") {
      return (
        env.npmCli ??
        process.env.npm_execpath ??
        path.join(path.dirname(process.execPath), "node_modules", "npm", "bin", "npm-cli.js")
      );
    }
    const bin = /^<bin:(.+)>$/.exec(arg)?.[1];
    if (bin === undefined) return arg;
    const dir = path.join(env.root, "node_modules", bin);
    const manifest = /** @type {{ bin?: string | Record<string, string> }} */ (
      readJson(path.join(dir, "package.json"))
    );
    const entry = typeof manifest.bin === "string" ? manifest.bin : manifest.bin?.[bin];
    if (entry === undefined) throw new Error(`${bin} has no "${bin}" bin`);
    return path.join(dir, entry);
  });
}

/**
 * Directories pip installs console scripts into (typos), which
 * are often not on PATH on Windows.
 *
 * @returns {string[]}
 */
export function pythonScriptDirs() {
  for (const python of ["python", "python3", "py"]) {
    const result = spawnSync(
      python,
      [
        "-c",
        "import os,sysconfig;print(sysconfig.get_path('scripts'));print(sysconfig.get_path('scripts', os.name + '_user'))",
      ],
      { encoding: "utf8" },
    );
    if (result.status === 0) return result.stdout.split(/\r?\n/).filter(Boolean);
  }
  return [];
}

/**
 * Finds an executable on PATH or in the extra directories.
 *
 * @param {string} tool
 * @param {{ path?: string, extraDirs?: string[], platform?: string, exists?: (p: string) => boolean }} [env]
 * @returns {string | null} the path, or null when not found
 */
export function resolveTool(tool, env = {}) {
  const platform = env.platform ?? process.platform;
  const exists = env.exists ?? ((p) => fs.existsSync(p) && fs.statSync(p).isFile());
  const dirs = [
    ...(env.path ?? process.env.PATH ?? "").split(path.delimiter),
    ...(env.extraDirs ?? []),
  ].filter(Boolean);
  const names =
    platform === "win32" ? [".exe", ".cmd", ".bat", ""].map((ext) => tool + ext) : [tool];
  for (const dir of dirs) {
    for (const name of names) {
      const candidate = path.join(dir, name);
      if (exists(candidate)) return candidate;
    }
  }
  return null;
}

/**
 * @typedef {{ id: string, title: string, status: "ok" | "failed" | "missing", seconds: number, note?: string }} StepResult
 */

/**
 * The summary table printed at the end and the overall exit code.
 *
 * @param {StepResult[]} results
 * @returns {{ text: string, exitCode: number }}
 */
export function summarize(results) {
  const width = Math.max(...results.map((r) => r.id.length), 4);
  const lines = results.map((r) => {
    const mark = r.status === "ok" ? "ok     " : r.status === "missing" ? "MISSING" : "FAILED ";
    const note = r.note ? `  (${r.note})` : "";
    return `  ${mark}  ${r.id.padEnd(width)}  ${r.seconds.toFixed(1).padStart(6)} s  ${r.title}${note}`;
  });
  const bad = results.filter((r) => r.status !== "ok");
  const headline =
    bad.length === 0
      ? `check: all ${String(results.length)} steps passed`
      : `check: ${String(bad.length)} of ${String(results.length)} steps failed: ${bad.map((r) => r.id).join(", ")}`;
  return { text: [headline, ...lines].join("\n"), exitCode: bad.length === 0 ? 0 : 1 };
}
