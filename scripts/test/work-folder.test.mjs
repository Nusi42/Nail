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
  folderOf,
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
