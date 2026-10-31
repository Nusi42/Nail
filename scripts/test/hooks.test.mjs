// Wiring of the hooks and local settings (ADRs 0123, 0132, 0205): the Git
// hooks, the Claude Code post-edit hook and the cargo include stay connected
// to the scripts that do the work, and the hooks behave as they should when
// git runs them (throw-away repositories, synthetic private patterns).
// Run: node --test scripts/test/
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";

import { AFTER_EDIT, checkoutOf, editedFile, relativeTo } from "../post-edit.mjs";
import { parseJson } from "../lib/json.mjs";
import { PATTERNS_ENV } from "../lib/private-patterns.mjs";
import { MAX_REPORT_LINES, shortReport } from "../lib/run-checks.mjs";
import { HOOKS_DIR, LOCAL_ENV } from "../lib/work-folder.mjs";
import { NETWORK_URL, ROOT as root, ZERO_SHA, git, initRepo, withTempDir } from "./temp-repo.mjs";

/** @param {string} file */
const read = (file) => fs.readFileSync(path.join(root, file), "utf8");

/** @param {string} hook */
const hookLines = (hook) => read(path.join(HOOKS_DIR, hook)).split(/\r?\n/);

/** How a hook names a script of its own checkout. */
const script = (/** @type {string} */ name) => `"$(dirname "$0")/../scripts/${name}"`;

test("the Git hooks run the guard, the per-file checks and the push plan of their checkout", () => {
  const preCommit = hookLines("pre-commit");
  assert.ok(preCommit.includes(`node ${script("check-private.mjs")} --staged || exit 1`));
  assert.ok(preCommit.includes(`exec node ${script("pre-commit.mjs")}`));
  // The message rules (ADR 0200) first, then the private-data guard (ADR 0192).
  const commitMsg = hookLines("commit-msg");
  assert.ok(commitMsg.includes(`node ${script("check-commit-msg.mjs")} "$1" || exit 1`));
  assert.ok(commitMsg.includes(`exec node ${script("check-private.mjs")} --message "$1"`));
  // One program decides what a push runs (ADR 0205).
  assert.ok(hookLines("pre-push").includes(`exec node ${script("pre-push.mjs")} "$@"`));
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

test("the checkout of an edited file is the nearest folder with .git and its after-edit module", () => {
  const base = fs.mkdtempSync(path.join(os.tmpdir(), "nail-post-edit-"));
  try {
    const work = path.join(base, "nail-work-3");
    fs.mkdirSync(path.join(work, ".git"), { recursive: true });
    fs.mkdirSync(path.join(work, "scripts", "lib"), { recursive: true });
    fs.writeFileSync(path.join(work, AFTER_EDIT), "");
    fs.mkdirSync(path.join(work, "ui", "src"), { recursive: true });
    assert.equal(checkoutOf(path.join(work, "ui", "src", "a.ts")), work);
    assert.equal(checkoutOf(path.join(work, "a.md")), work);
    // A plain git repository without that module is not a Nail checkout.
    fs.mkdirSync(path.join(base, "other", ".git"), { recursive: true });
    assert.equal(checkoutOf(path.join(base, "other", "x.ts")), undefined);
  } finally {
    fs.rmSync(base, { recursive: true, force: true });
  }
});

test("the post-edit hook hands the file to the edited checkout's own code", () => {
  // The hook file may come from an older checkout (the master): what runs is
  // the after-edit module of the checkout that holds the file.
  withTempDir("nail-post-edit-", (work) => {
    fs.mkdirSync(path.join(work, ".git"));
    fs.mkdirSync(path.join(work, "scripts", "lib"), { recursive: true });
    fs.writeFileSync(
      path.join(work, AFTER_EDIT),
      "export const afterEdit = async (root, file, rel) => `this checkout's code: ${rel}`;\n",
    );
    fs.mkdirSync(path.join(work, "docs"));
    const file = path.join(work, "docs", "a.md");
    fs.writeFileSync(file, "x\n");
    const run = spawnSync(process.execPath, [path.join(root, "scripts", "post-edit.mjs")], {
      input: JSON.stringify({ tool_input: { file_path: file } }),
      encoding: "utf8",
    });
    assert.equal(run.status, 2, run.stderr);
    assert.equal(run.stderr.trim(), "this checkout's code: docs/a.md");
  });
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

/**
 * A master and a work folder cloned from it, both using this checkout's
 * hooks, and an environment with synthetic private patterns.
 *
 * @param {string} base
 */
function hookedRepos(base) {
  const patterns = path.join(base, "patterns.txt");
  fs.writeFileSync(patterns, "\\bjoanne\\b\n");
  const hooks = path.join(root, HOOKS_DIR);
  const master = initRepo(path.join(base, "master"));
  fs.writeFileSync(path.join(master, LOCAL_ENV), "NAIL_CHECKOUT_ROLE=master\n");
  git(master, ["config", "core.hooksPath", hooks]);
  const work = path.join(base, "work");
  git(base, ["clone", "-q", master, work]);
  for (const [key, value] of /** @type {[string, string][]} */ ([
    ["user.email", "sam@example.net"],
    ["user.name", "Sam"],
    ["core.hooksPath", hooks],
  ])) {
    git(work, ["config", key, value]);
  }
  /** @type {NodeJS.ProcessEnv} */
  const env = { ...process.env, [PATTERNS_ENV]: patterns };
  delete env.NAIL_CHECKOUT_ROLE;
  /** git with the hooks on, its result unchecked. */
  const hooked = (/** @type {string} */ dir, /** @type {string[]} */ args) =>
    spawnSync("git", args, { cwd: dir, env, encoding: "utf8" });
  /** Commits a file with a private value, the hooks off. */
  const commitPrivate = (/** @type {string} */ dir) => {
    fs.writeFileSync(path.join(dir, "p.txt"), "to joanne\n");
    git(dir, ["add", "p.txt"]);
    git(dir, ["-c", "core.hooksPath=no-hooks", "commit", "-q", "-m", "feat: p"]);
    return git(dir, ["rev-parse", "HEAD"]).trim();
  };
  /** Runs the pre-push hook as git would, with these ref lines on stdin. */
  const prePush = (
    /** @type {string} */ dir,
    /** @type {string} */ url,
    /** @type {string} */ lines,
  ) => {
    const stdin = path.join(base, "push-lines.txt");
    fs.writeFileSync(stdin, lines);
    return hooked(dir, ["hook", "run", `--to-stdin=${stdin}`, "pre-push", "--", "github", url]);
  };
  return { master, work, hooked, commitPrivate, prePush };
}

test("pre-commit: a staged file with a private value fails the commit", () => {
  withTempDir("nail-hooks-", (base) => {
    const { work, hooked } = hookedRepos(base);
    fs.writeFileSync(path.join(work, "b.txt"), "mail joanne\n");
    git(work, ["add", "b.txt"]);
    const commit = hooked(work, ["commit", "-q", "-m", "feat: b"]);
    assert.notEqual(commit.status, 0);
    assert.match(commit.stderr, /^b\.txt:1:\d+: private pattern 1/m);
    assert.equal(git(work, ["log", "--format=%s", "-1"]).trim(), "clean");
  });
});

test("pre-push: a work folder's push to the master runs the guard on the pushed commits", () => {
  withTempDir("nail-hooks-", (base) => {
    const { master, work, hooked, commitPrivate } = hookedRepos(base);
    commitPrivate(work);
    const push = hooked(work, ["push", "origin", "HEAD:refs/heads/work-1"]);
    assert.notEqual(push.status, 0);
    assert.match(push.stderr, /private pattern 1/);
    assert.match(push.stderr, /pre-push: private-push failed/);
    assert.equal(git(master, ["branch", "--list", "work-1"]), "");
  });
});

test("pre-push: a merge commit never reaches the master (ADR 0208)", () => {
  withTempDir("nail-hooks-", (base) => {
    const { master, work, hooked } = hookedRepos(base);
    const off = ["-c", "core.hooksPath=no-hooks"];
    git(work, ["switch", "-q", "-c", "side"]);
    fs.writeFileSync(path.join(work, "s.txt"), "side\n");
    git(work, ["add", "s.txt"]);
    git(work, [...off, "commit", "-q", "-m", "feat: side"]);
    git(work, ["switch", "-q", "main"]);
    fs.writeFileSync(path.join(work, "m.txt"), "main\n");
    git(work, ["add", "m.txt"]);
    git(work, [...off, "commit", "-q", "-m", "feat: main"]);
    git(work, [...off, "merge", "-q", "--no-ff", "-m", "Merge the side", "side"]);
    const push = hooked(work, ["push", "origin", "HEAD:refs/heads/work-1"]);
    assert.notEqual(push.status, 0);
    assert.match(push.stderr, /^merge commit [0-9a-f]+ Merge the side$/m);
    assert.match(push.stderr, /pre-push: linear failed/);
    assert.equal(git(master, ["branch", "--list", "work-1"]), "");
  });
});

test("pre-push: a work folder never pushes to a network remote", () => {
  withTempDir("nail-hooks-", (base) => {
    const { master, work, prePush } = hookedRepos(base);
    const head = git(work, ["rev-parse", "HEAD"]).trim();
    const run = prePush(work, NETWORK_URL, `refs/heads/main ${head} refs/heads/main ${ZERO_SHA}\n`);
    assert.equal(run.status, 1);
    assert.match(run.stderr, /a work folder pushes only to the master/);
    assert.ok(run.stderr.includes(`git -C "${path.resolve(master)}" push github main`));
  });
});

test("pre-push: the master never pushes to a local path", () => {
  withTempDir("nail-hooks-", (base) => {
    const { master, hooked } = hookedRepos(base);
    const bare = path.join(base, "bare.git");
    git(base, ["init", "-q", "--bare", bare]);
    const push = hooked(master, ["push", bare, "main"]);
    assert.notEqual(push.status, 0);
    assert.match(push.stderr, /the master pushes only to the public remote/);
    assert.equal(git(bare, ["branch", "--list"]), "");
  });
});

test("pre-push: the master publishes only main, and the guard checks what it pushes", () => {
  withTempDir("nail-hooks-", (base) => {
    const { master, commitPrivate, prePush } = hookedRepos(base);
    const head = git(master, ["rev-parse", "HEAD"]).trim();
    const other = prePush(master, NETWORK_URL, `refs/heads/x ${head} refs/heads/x ${ZERO_SHA}\n`);
    assert.equal(other.status, 1);
    assert.match(other.stderr, /the master publishes only main/);

    const sha = commitPrivate(master);
    const main = prePush(
      master,
      NETWORK_URL,
      `refs/heads/main ${sha} refs/heads/main ${ZERO_SHA}\n`,
    );
    assert.equal(main.status, 1);
    assert.match(main.stderr, /private pattern 1/);
    assert.match(main.stderr, /pre-push: private-push failed/);
  });
});
