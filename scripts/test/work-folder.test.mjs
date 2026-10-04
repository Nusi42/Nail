// Tests of scripts/lib/work-folder.mjs (master repository and work folders). Run: node --test scripts/test/
import assert from "node:assert/strict";
import path from "node:path";
import { test } from "node:test";

import {
  MASTER_RUSTC_WRAPPER,
  branchOf,
  claudeLocalSettings,
  folderOf,
  isInOneDrive,
  masterCargoConfig,
  masterRefusal,
  parseFolderNumber,
} from "../lib/work-folder.mjs";
import { parseJson } from "../lib/json.mjs";

test("work folder n is nail-work-<N> on branch work-<N>", () => {
  assert.equal(folderOf(1), "nail-work-1");
  assert.equal(branchOf(12), "work-12");
});

test("folder numbers are 1-99", () => {
  assert.equal(parseFolderNumber("3"), 3);
  assert.equal(parseFolderNumber("42"), 42);
  for (const bad of [undefined, "", "0", "100", "-1", "2a", " 2"]) {
    assert.throws(() => parseFolderNumber(bad), /1-99/);
  }
});

test("OneDrive folders are recognised by a path segment", () => {
  assert.equal(isInOneDrive("C:\\Users\\x\\OneDrive\\<projects>\\nail"), true);
  assert.equal(isInOneDrive("C:/Users/x/onedrive/nail"), true);
  assert.equal(isInOneDrive("C:\\Users\\x\\OneDrive - Company\\nail"), true);
  assert.equal(isInOneDrive("C:\\Users\\x\\<work root>\\nail-work-1"), false);
  assert.equal(isInOneDrive("C:\\Users\\x\\NotOneDrive\\nail"), false);
  assert.equal(isInOneDrive("C:\\Users\\x\\OneDriveBackup\\nail"), false);
});

test("the master refuses development commands and names the work folders", () => {
  const refusal = masterRefusal("C:\\Users\\x\\OneDrive\\<projects>\\nail", "npm run check");
  assert.ok(refusal !== undefined);
  assert.ok(refusal.startsWith("npm run check: refused"));
  assert.match(refusal, /<projects>\.InWork\\nail\.work-1 \(branch work-1\)/);
  assert.match(refusal, /work-folders\.mjs create <n>/);
  assert.equal(
    masterRefusal("C:\\Users\\x\\<work root>\\nail-work-2", "npm run check"),
    undefined,
  );
});

test("the master's cargo config names the refusing wrapper", () => {
  const config = masterCargoConfig(path.join(".cargo", "master-guard.cmd"));
  assert.match(config, /^\[build\]$/m);
  assert.match(config, /^rustc-wrapper = "\.cargo\/master-guard\.cmd"$/m);
  assert.match(MASTER_RUSTC_WRAPPER, /^@echo off\r\n/);
  assert.match(MASTER_RUSTC_WRAPPER, /\r\nexit \/b 1\r\n$/);
});

test("a work folder keeps Claude Code's memory in its checkout", () => {
  const settings = parseJson(claudeLocalSettings("C:\\w\\nail-work-3"));
  assert.deepEqual(settings, { autoMemoryDirectory: "C:/w/nail-work-3/.claude/memory" });
});
