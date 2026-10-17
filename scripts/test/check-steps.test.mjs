// Tests of scripts/lib/check-steps.mjs and scripts/lib/staged.mjs: the
// `npm run check` steps and the pre-commit checks (ADR 0123).
// Run: node --test "scripts/test/*.test.mjs"
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { test } from "node:test";

import {
  checkSteps,
  parseCheckArgs,
  resolveArgs,
  resolveTool,
  selectSteps,
  summarize,
} from "../lib/check-steps.mjs";
import { chunkFiles, hookChecks, parseStaged } from "../lib/staged.mjs";

const root = path.resolve(import.meta.dirname, "../..");

/**
 * @param {string} file
 * @returns {unknown}
 */
const readJson = (file) => JSON.parse(fs.readFileSync(file, "utf8"));

/** @param {string} id */
const step = (id) => {
  const found = checkSteps().find((s) => s.id === id);
  assert.ok(found, `step ${id}`);
  return found;
};

test("check covers every formatter and linter the quality baseline names", () => {
  const ids = checkSteps().map((s) => s.id);
  for (const id of [
    "rustfmt",
    "taplo-fmt",
    "taplo-lint",
    "prettier",
    "editorconfig",
    "private",
    "typos",
    "markdownlint",
    "eslint",
    "tsc",
    "clippy",
    "rustdoc",
    "machete",
    "deny",
    "duplicates",
  ]) {
    assert.ok(ids.includes(id), `missing step ${id}`);
  }
  assert.equal(new Set(ids).size, ids.length, "step ids are unique");
});

test("warnings are errors: clippy, rustdoc and ESLint", () => {
  const clippy = step("clippy").args;
  assert.deepEqual(clippy.slice(clippy.indexOf("--")), ["--", "-D", "warnings"]);
  assert.ok(clippy.includes("--all-targets") && clippy.includes("--workspace"));
  assert.equal(step("rustdoc").env?.RUSTDOCFLAGS, "-D warnings");
  assert.deepEqual(step("rustdoc").args.slice(-2), ["--workspace", "--no-deps"]);
  const eslint = step("eslint").args;
  assert.equal(eslint[eslint.indexOf("--max-warnings") + 1], "0");
});

test("cargo runs with a limited job count (shared machine)", () => {
  assert.deepEqual(step("clippy").args.slice(0, 3), ["clippy", "-j", "4"]);
  const two = checkSteps({ jobs: "2" }).find((s) => s.id === "rustdoc");
  assert.deepEqual(two?.args.slice(0, 3), ["doc", "-j", "2"]);
});

test("cargo deny checks advisories only on request or with CI=1", () => {
  const offline = step("deny").args;
  assert.deepEqual(offline, ["--locked", "check", "licenses", "bans", "sources"]);
  for (const options of [{ ci: true }, { advisories: true }]) {
    assert.deepEqual(checkSteps(options).find((s) => s.id === "deny")?.args, ["--locked", "check"]);
  }
});

test("every npm script a step runs exists", () => {
  const pkg = /** @type {{ scripts: Record<string, string | undefined> }} */ (
    readJson(path.join(root, "package.json"))
  );
  assert.equal(pkg.scripts.check, "node scripts/check.mjs");
  assert.ok(pkg.scripts.prepare?.includes("install-hooks.mjs"));
  for (const s of checkSteps()) {
    if (s.args[0] === "<npm>")
      assert.ok(pkg.scripts[s.args[2] ?? ""], `${s.id}: ${s.args.join(" ")}`);
  }
});

test("selectSteps: --only, --skip and unknown ids", () => {
  const steps = checkSteps();
  assert.deepEqual(
    selectSteps(steps, { only: ["eslint", "typos"] }).map((s) => s.id),
    ["typos", "eslint"],
  );
  assert.ok(!selectSteps(steps, { skip: ["clippy"] }).some((s) => s.id === "clippy"));
  assert.throws(() => selectSteps(steps, { only: ["nope"] }), /unknown check step/);
});

test("parseCheckArgs", () => {
  assert.deepEqual(
    parseCheckArgs(["--only", "a,b", "--skip", "c", "--advisories", "--fail-fast"]),
    {
      only: ["a", "b"],
      skip: ["c"],
      advisories: true,
      list: false,
      keepGoing: false,
    },
  );
  assert.throws(() => parseCheckArgs(["--only"]), /needs a value/);
  assert.throws(() => parseCheckArgs(["--bogus"]), /unknown argument/);
});

test("resolveTool searches PATH and extra directories with Windows extensions", () => {
  const present = new Set([
    path.join("C:/py/Scripts", "typos.exe"),
    path.join("/usr/bin", "taplo"),
  ]);
  const exists = (/** @type {string} */ p) => present.has(p);
  assert.equal(
    resolveTool("typos", {
      path: "C:/bin",
      extraDirs: ["C:/py/Scripts"],
      platform: "win32",
      exists,
    }),
    path.join("C:/py/Scripts", "typos.exe"),
  );
  assert.equal(
    resolveTool("taplo", {
      path: ["/bin", "/usr/bin"].join(path.delimiter),
      platform: "linux",
      exists,
    }),
    path.join("/usr/bin", "taplo"),
  );
  assert.equal(resolveTool("no-such-tool", { path: "", platform: "win32", exists }), null);
});

test("resolveArgs turns <bin:pkg> and <npm> into script paths", () => {
  /** @type {Record<string, unknown>} */
  const manifests = {
    [path.join("/repo", "node_modules", "prettier", "package.json")]: { bin: "./bin/prettier.cjs" },
    [path.join("/repo", "node_modules", "eslint", "package.json")]: {
      bin: { eslint: "./bin/eslint.js" },
    },
  };
  const readJson = (/** @type {string} */ file) => manifests[file];
  assert.deepEqual(
    resolveArgs(["<bin:prettier>", "--check", "<bin:eslint>", "<npm>"], {
      root: "/repo",
      npmCli: "/npm/npm-cli.js",
      readJson,
    }),
    [
      path.join("/repo", "node_modules", "prettier", "bin", "prettier.cjs"),
      "--check",
      path.join("/repo", "node_modules", "eslint", "bin", "eslint.js"),
      "/npm/npm-cli.js",
    ],
  );
  // The real packages: every <bin:...> a step or hook uses is installed.
  const used = [...checkSteps(), ...hookChecks(["a.ts", "a.md"])].flatMap((s) =>
    s.args.filter((a) => a.startsWith("<bin:")),
  );
  for (const resolved of resolveArgs(used, { root })) assert.ok(fs.existsSync(resolved), resolved);
});

test("summarize lists every step and fails on a failed or missing tool", () => {
  const ok = summarize([{ id: "a", title: "A", status: "ok", seconds: 1 }]);
  assert.equal(ok.exitCode, 0);
  assert.match(ok.text, /all 1 steps passed/);
  const bad = summarize([
    { id: "a", title: "A", status: "ok", seconds: 1 },
    { id: "typos", title: "T", status: "missing", seconds: 0, note: "pip install" },
    { id: "clippy", title: "C", status: "failed", seconds: 9 },
  ]);
  assert.equal(bad.exitCode, 1);
  assert.match(bad.text, /2 of 3 steps failed: typos, clippy/);
  assert.match(bad.text, /MISSING\s+typos/);
});

test("pre-commit: checks per staged file kind", () => {
  const files = parseStaged(
    "crates/a/src/lib.rs\0ui/src/App.tsx\0Cargo.toml\0docs/x.md\0logo.png\0",
  );
  assert.deepEqual(files, [
    "crates/a/src/lib.rs",
    "ui/src/App.tsx",
    "Cargo.toml",
    "docs/x.md",
    "logo.png",
  ]);
  const byId = Object.fromEntries(hookChecks(files).map((c) => [c.id, c.files]));
  assert.deepEqual(byId.rustfmt, ["crates/a/src/lib.rs"]);
  assert.deepEqual(byId.duplicates, ["crates/a/src/lib.rs", "ui/src/App.tsx"]);
  assert.deepEqual(byId.eslint, ["ui/src/App.tsx"]);
  assert.deepEqual(byId.prettier, ["ui/src/App.tsx"]);
  assert.deepEqual(byId["taplo-fmt"], ["Cargo.toml"]);
  assert.deepEqual(byId.markdownlint, ["docs/x.md"]);
  assert.deepEqual(byId.typos, files);
  assert.deepEqual(byId.editorconfig, files);
});

test("pre-commit: nothing staged of a kind means no check for it", () => {
  assert.deepEqual(
    hookChecks(["README.md"]).map((c) => c.id),
    ["markdownlint", "typos", "editorconfig"],
  );
  assert.deepEqual(hookChecks([]), []);
  const eslint = hookChecks(["a.mjs"]).find((c) => c.id === "eslint");
  assert.ok(eslint?.args.includes("--max-warnings") && eslint.args.includes("0"));
});

test("chunkFiles keeps command lines short", () => {
  const files = Array.from({ length: 10 }, (_, i) => `file-${String(i)}.ts`);
  const chunks = chunkFiles(files, 30);
  assert.deepEqual(chunks.flat(), files);
  for (const chunk of chunks) assert.ok(chunk.join(" ").length <= 30 || chunk.length === 1);
  assert.deepEqual(chunkFiles([]), []);
});

test("the pre-commit hook runs the staged checks and is executable in git", () => {
  const hook = fs.readFileSync(path.join(root, ".githooks", "pre-commit"), "utf8");
  assert.match(hook, /^#!\/bin\/sh\n/);
  assert.match(hook, /node scripts\/pre-commit\.mjs/);
});

test(".prettierignore skips generated crate data but formats hand-written crate code (Q1 review)", async () => {
  const prettier = await import("prettier");
  const ignorePath = path.join(root, ".prettierignore");
  /** @param {string} file */
  const ignored = async (file) =>
    (await prettier.getFileInfo(path.join(root, file), { ignorePath })).ignored;
  assert.equal(await ignored("crates/nail-plugins/src/prelude.js"), false);
  assert.equal(await ignored("crates/nail-api/fixtures/default-theme.json"), true);
  assert.equal(await ignored("crates/nail-proto-gmail/fixtures/responses/x.json"), true);
  assert.equal(await ignored("crates/nail-config/schema/theme.schema.json"), true);
  assert.equal(await ignored("ui/src/main.tsx"), false);
});

test("the Vite build fails on warnings (Q1 review, ADR 0130)", () => {
  const config = fs.readFileSync(path.join(root, "ui", "vite.config.ts"), "utf8");
  assert.match(config, /strictBuild\(\)/);
  // Relative imports carry their extension: Vite warns about the config
  // otherwise, before the strict logger exists.
  for (const m of config.matchAll(/from "(\.[^"]*)"/g)) assert.match(m[1] ?? "", /\.ts$/);
  assert.match(config, /customLogger: strict\.logger/);
  assert.match(config, /strict\.plugin/);
});
