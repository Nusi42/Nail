// Tests of scripts/lib/work-folder.mjs (master repository and work folders). Run: node --test scripts/test/
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";

import {
  HOOKS_DIR,
  HOOKS_PATH_ARGS,
  LOCAL_ENV,
  MASTER_RUSTC_WRAPPER,
  REPO_ROOT,
  branchOf,
  claudeLocalSettings,
  cleanWorkFolder,
  folderOf,
  gitOut,
  guardMaster,
  identityConfig,
  isMaster,
  localSettings,
  markedAsMaster,
  masterCargoConfig,
  masterRefusal,
  parseFolderNumber,
} from "../lib/work-folder.mjs";
import { parseJson } from "../lib/json.mjs";
import { git, initRepo, withTempDir } from "./temp-repo.mjs";

test("a new work folder takes the master's own commit identity, nothing else", () => {
  const master = new Map([
    ["user.name", "Sam"],
    ["user.email", "sam@example.net"],
  ]);
  assert.deepEqual(
    identityConfig((key) => master.get(key)),
    [
      ["config", "user.name", "Sam"],
      ["config", "user.email", "sam@example.net"],
    ],
  );
  assert.deepEqual(
    identityConfig(() => undefined),
    [],
  );
});

test("work folders are nail-work-<N> on work-<N> unless configured otherwise", () => {
  assert.equal(folderOf(1), "nail-work-1");
  assert.equal(branchOf(12), "work-12");
  const settings = { NAIL_WORK_FOLDER_PREFIX: "mail-", NAIL_WORK_BRANCH_PREFIX: "lane-" };
  assert.equal(folderOf(2, settings), "mail-2");
  assert.equal(branchOf(2, settings), "lane-2");
});

test("folder numbers are 1-99", () => {
  assert.equal(parseFolderNumber("3"), 3);
  assert.equal(parseFolderNumber("42"), 42);
  for (const bad of [undefined, "", "0", "100", "-1", "2a", " 2"]) {
    assert.throws(() => parseFolderNumber(bad), /1-99/);
  }
});

test("only a checkout marked as the master refuses development commands", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "nail-master-"));
  try {
    // A plain clone (any path, OneDrive or not) is a work folder.
    assert.equal(isMaster(dir, {}), false);
    assert.equal(masterRefusal(dir, "npm run check", {}), undefined);
    // Marked in its .env.local, other settings kept.
    const env = path.join(dir, ".env.local");
    fs.writeFileSync(
      env,
      markedAsMaster("NAIL_WORK_FOLDER_PREFIX=mail-\nNAIL_CHECKOUT_ROLE=work\n"),
    );
    assert.equal(
      fs.readFileSync(env, "utf8"),
      "NAIL_WORK_FOLDER_PREFIX=mail-\nNAIL_CHECKOUT_ROLE=master\n",
    );
    assert.equal(localSettings(dir, {}).NAIL_WORK_FOLDER_PREFIX, "mail-");
    assert.equal(isMaster(dir, {}), true);
    const refusal = masterRefusal(dir, "npm run check", {});
    assert.ok(refusal !== undefined);
    assert.ok(refusal.startsWith("npm run check: refused"));
    assert.match(refusal, /work folder \(mail-1 and up\)/);
    assert.match(refusal, /work-folders\.mjs create <n>/);
    // The environment wins over the file.
    assert.equal(isMaster(dir, { NAIL_CHECKOUT_ROLE: "work" }), false);
    assert.equal(markedAsMaster(""), "NAIL_CHECKOUT_ROLE=master\n");
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("the master's cargo config names the refusing wrapper", () => {
  const config = masterCargoConfig(path.join(".cargo", "master-guard.cmd"));
  assert.match(config, /^\[build\]$/m);
  assert.match(config, /^rustc-wrapper = "\.cargo\/master-guard\.cmd"$/m);
  assert.match(MASTER_RUSTC_WRAPPER, /^@echo off\r\n/);
  // Every compilation is refused; only cargo's version and target questions
  // (cargo metadata, cargo deny) pass.
  assert.match(MASTER_RUSTC_WRAPPER, /\r\nexit \/b 1\r\n:query\r\n%\*\r\n$/);
  const passes = MASTER_RUSTC_WRAPPER.split("\r\n").filter((l) => l.endsWith(" goto query"));
  assert.deepEqual(passes, [
    'if "%~2"=="-vV" goto query',
    'if "%~2 %~3 %~4"=="- --crate-name ___" goto query',
  ]);
});

test("marking the master writes its role and cargo guard and turns its hooks on", () => {
  withTempDir("nail-guard-", (dir) => {
    const master = initRepo(path.join(dir, "master"));
    fs.writeFileSync(path.join(master, LOCAL_ENV), "NAIL_WORK_BRANCH_PREFIX=WIN\n");
    assert.equal(guardMaster(master), path.join(master, LOCAL_ENV));
    assert.equal(isMaster(master, {}), true);
    assert.equal(localSettings(master, {}).NAIL_WORK_BRANCH_PREFIX, "WIN");
    assert.equal(git(master, ["config", "core.hooksPath"]).trim(), HOOKS_DIR);
    assert.equal(
      fs.readFileSync(path.join(master, ".cargo", "master-guard.cmd"), "utf8"),
      MASTER_RUSTC_WRAPPER,
    );
    assert.match(
      fs.readFileSync(path.join(master, ".cargo", "config.local.toml"), "utf8"),
      /^rustc-wrapper = "\.cargo\/master-guard\.cmd"$/m,
    );
  });
});

test("a work folder keeps Claude Code's memory in its checkout", () => {
  const settings = parseJson(claudeLocalSettings("C:\\w\\nail-work-3"));
  assert.deepEqual(settings, { autoMemoryDirectory: "C:/w/nail-work-3/.claude/memory" });
});

test("hooks: install-hooks and new work folders point git at one directory", () => {
  assert.equal(HOOKS_DIR, ".githooks");
  assert.deepEqual(HOOKS_PATH_ARGS, ["config", "core.hooksPath", HOOKS_DIR]);
  assert.ok(fs.existsSync(path.join(REPO_ROOT, HOOKS_DIR, "pre-commit")));
});

test("cleaning an idle work folder resets it to main and keeps only its local settings", () => {
  withTempDir("nail-clean-", (dir) => {
    const master = initRepo(path.join(dir, "master"));
    const work = path.join(dir, "work-2");
    git(dir, ["clone", "-q", master, work]);
    for (const args of identityConfig((key) => gitOut(master, ["config", key]))) git(work, args);
    git(work, ["config", "core.hooksPath", "no-hooks"]);
    git(work, ["switch", "-q", "-c", "work-2"]);
    fs.writeFileSync(
      path.join(work, ".git", "info", "exclude"),
      ".env.local\n.claude/\ntarget/\n*.log\n",
    );
    const kept = [".env.local", path.join(".claude", "settings.local.json")];
    for (const file of [...kept, path.join("target", "debug.txt"), "scratch.log"]) {
      fs.mkdirSync(path.dirname(path.join(work, file)), { recursive: true });
      fs.writeFileSync(path.join(work, file), "x\n");
    }
    /**
     * Asserts that cleaning on `branch` is refused and changes nothing.
     *
     * @param {string} branch
     * @param {RegExp} reason
     */
    const refused = (branch, reason) => {
      const before = [gitOut(work, ["rev-parse", "HEAD"]), fs.readdirSync(work).sort()];
      assert.throws(() => {
        cleanWorkFolder(work, branch);
      }, reason);
      assert.deepEqual([gitOut(work, ["rev-parse", "HEAD"]), fs.readdirSync(work).sort()], before);
    };

    refused("work-3", /is on work-2, not work-3/);
    fs.writeFileSync(path.join(work, "new.txt"), "new work\n");
    refused("work-2", /uncommitted changes/);
    fs.rmSync(path.join(work, "new.txt"));
    fs.writeFileSync(path.join(work, "a.txt"), "changed\n");
    refused("work-2", /uncommitted changes/);
    git(work, ["commit", "-q", "-am", "package"]);
    refused("work-2", /commits neither in main nor pushed/);

    // Pushed: the master keeps the commit, so the folder may go back to main.
    git(work, ["push", "-q", "origin", "work-2"]);
    const pushed = gitOut(work, ["rev-parse", "HEAD"]);
    cleanWorkFolder(work, "work-2");
    const main = () => gitOut(master, ["rev-parse", "main"]);
    assert.equal(gitOut(work, ["rev-parse", "HEAD"]), main());
    assert.equal(gitOut(master, ["rev-parse", "work-2"]), pushed);
    assert.ok(!fs.existsSync(path.join(work, "target")));
    assert.ok(!fs.existsSync(path.join(work, "scratch.log")));
    for (const file of kept) assert.ok(fs.existsSync(path.join(work, file)), file);

    // Not pushed, but its change reached main as a different commit (a rebase).
    fs.writeFileSync(path.join(work, "b.txt"), "next\n");
    git(work, ["add", "b.txt"]);
    git(work, ["commit", "-q", "-m", "next package"]);
    refused("work-2", /commits neither in main nor pushed/);
    fs.writeFileSync(path.join(master, "b.txt"), "next\n");
    git(master, ["add", "b.txt"]);
    git(master, ["commit", "-q", "-m", "next package, rebased"]);
    cleanWorkFolder(work, "work-2");
    assert.equal(gitOut(work, ["rev-parse", "HEAD"]), main());
  });
});
