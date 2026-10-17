// Wiring of the hooks and local settings (ADRs 0123, 0132): the Git hooks,
// the Claude Code post-edit hook and the cargo include stay connected to the
// scripts that do the work. Run: node --test scripts/test/
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { checkoutOf, editedFile, relativeTo } from "../post-edit.mjs";
import { parseJson } from "../lib/json.mjs";
import { MAX_REPORT_LINES, shortReport } from "../lib/run-checks.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");

/** @param {string} file */
const read = (file) => fs.readFileSync(path.join(root, file), "utf8");

test("the Git hooks run the pre-commit checks and `npm run check` before a push", () => {
  assert.match(read(".githooks/pre-commit"), /^exec node scripts\/pre-commit\.mjs$/m);
  assert.match(read(".githooks/pre-push"), /^exec node scripts\/check\.mjs$/m);
});

test("the Git hooks run the private-data guard (ADR 0192)", () => {
  assert.match(
    read(".githooks/pre-commit"),
    /^node scripts\/check-private\.mjs --staged \|\| exit 1$/m,
  );
  assert.match(
    read(".githooks/commit-msg"),
    /^exec node scripts\/check-private\.mjs --message "\$1"$/m,
  );
  assert.match(
    read(".githooks/pre-push"),
    /^node scripts\/check-private\.mjs --push \|\| exit 1$/m,
  );
});

test("the commit-msg hook checks the message rules first (ADR 0200)", () => {
  assert.match(
    read(".githooks/commit-msg"),
    /^node scripts\/check-commit-msg\.mjs "\$1" \|\| exit 1$/m,
  );
});

test("the Claude Code post-edit hook runs scripts/post-edit.mjs of the session folder", () => {
  /** @typedef {{ type: string, command: string }} Hook */
  const settings = /** @type {{ hooks: { PostToolUse: { matcher: string, hooks: Hook[] }[] } }} */ (
    parseJson(read(".claude/settings.json"))
  );
  const [entry] = settings.hooks.PostToolUse;
  assert.ok(entry);
  assert.equal(entry.matcher, "Edit|Write|MultiEdit|NotebookEdit");
  const [hook] = entry.hooks;
  assert.ok(hook);
  assert.equal(hook.type, "command");
  // The session runs in the master; the script finds the work folder itself.
  assert.equal(hook.command, 'node "$CLAUDE_PROJECT_DIR/scripts/post-edit.mjs"');
});

test("cargo reads machine-specific settings from the ignored config.local.toml", () => {
  assert.match(
    read(".cargo/config.toml"),
    /^include = \[\{ path = "config\.local\.toml", optional = true \}\]$/m,
  );
  const ignored = read(".gitignore").split(/\r?\n/);
  assert.ok(ignored.includes("/.cargo/config.local.toml"));
  assert.ok(ignored.includes("/.cargo/master-guard.cmd"));
  assert.ok(!ignored.includes("/.cargo/config.toml"));
});

test("the post-edit hook reads the edited file from the tool call", () => {
  assert.equal(editedFile('{"tool_input":{"file_path":"C:\\\\w\\\\a.ts"}}'), "C:\\w\\a.ts");
  assert.equal(editedFile('{"tool_input":{"notebook_path":"n.ipynb"}}'), "n.ipynb");
  assert.equal(editedFile('{"tool_input":{"command":"ls"}}'), undefined);
  assert.equal(editedFile('{"tool_name":"Bash"}'), undefined);
  assert.equal(editedFile("null"), undefined);
});

test("the checkout of an edited file is the nearest folder with .git and the check runner", () => {
  const base = fs.mkdtempSync(path.join(os.tmpdir(), "nail-post-edit-"));
  try {
    const work = path.join(base, "nail-work-3");
    fs.mkdirSync(path.join(work, ".git"), { recursive: true });
    fs.mkdirSync(path.join(work, "scripts", "lib"), { recursive: true });
    fs.writeFileSync(path.join(work, "scripts", "lib", "run-checks.mjs"), "");
    fs.mkdirSync(path.join(work, "ui", "src"), { recursive: true });
    assert.equal(checkoutOf(path.join(work, "ui", "src", "a.ts")), work);
    assert.equal(checkoutOf(path.join(work, "a.md")), work);
    // A plain git repository without Nail's runner is not a Nail checkout.
    fs.mkdirSync(path.join(base, "other", ".git"), { recursive: true });
    assert.equal(checkoutOf(path.join(base, "other", "x.ts")), undefined);
  } finally {
    fs.rmSync(base, { recursive: true, force: true });
  }
});

test("only files inside the checkout are checked, with / separators", () => {
  const base = path.join(path.sep, "w", "nail-work-2");
  assert.equal(relativeTo(base, path.join(base, "ui", "src", "a.ts")), "ui/src/a.ts");
  assert.equal(relativeTo(base, path.join(path.sep, "w", "other", "a.ts")), undefined);
  assert.equal(relativeTo(base, base), undefined);
});

test("the post-edit report keeps only failed checks and stays short", () => {
  const check = (/** @type {string} */ id) => ({
    id,
    tool: "node",
    args: [],
    files: [],
    install: "",
  });
  const results = [
    { check: check("prettier"), code: 0, output: "all good\n" },
    { check: check("eslint"), code: 1, output: "a.ts\n\n  1:1 error x\n" },
  ];
  assert.equal(shortReport(results), "eslint failed:\na.ts\n  1:1 error x");
  const long = [{ check: check("typos"), code: 2, output: "x\n".repeat(50) }];
  const lines = shortReport(long).split("\n");
  assert.equal(lines.length, MAX_REPORT_LINES + 1);
  assert.equal(lines.at(-1), "...");
});
