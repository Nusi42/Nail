// Tests of scripts/lib/commit-message.mjs and scripts/check-commit-msg.mjs
// (ADR 0200). Run: node --test "scripts/test/*.test.mjs"
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { MAX_SUBJECT, commitMessageProblems, messageText } from "../lib/commit-message.mjs";

const SCRIPT = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
  "check-commit-msg.mjs",
);

const GOOD = [
  "feat(mail): create, rename, move and delete folders",
  "",
  "Folder changes are queued and replayed on the server.",
  "",
  "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>",
].join("\n");

test("Conventional Commits subjects and merges pass", () => {
  assert.deepEqual(commitMessageProblems(GOOD), []);
  assert.deepEqual(commitMessageProblems("fix: keep the user's junk verdict"), []);
  assert.deepEqual(commitMessageProblems("feat(ui)!: drop the old layout format"), []);
  assert.deepEqual(commitMessageProblems("Merge the folder management work"), []);
});

test("a subject without a type, or too long, is refused", () => {
  assert.match(commitMessageProblems("Add folder management")[0] ?? "", /<type>\(<scope>\)/);
  assert.match(commitMessageProblems("Feat: capital type")[0] ?? "", /<type>/);
  const long = `feat: ${"x".repeat(MAX_SUBJECT)}`;
  assert.match(commitMessageProblems(long).join("\n"), /characters long/);
});

test("session lines, claude.ai links and internal vocabulary are refused", () => {
  /** @type {[string, RegExp][]} */
  const cases = [
    ["feat: x\n\nClaude-Session: https://example.org/s", /session trailer/],
    // Built at runtime: the repository itself never holds such a link.
    [`docs: x\n\nSee https://${["claude", "ai"].join(".")}/code/artifact/1`, /claude\.ai/],
    ["chore: merge work-4 into the lead", /work-folder or branch/],
    ["chore: reset nail-work-2", /work-folder or branch/],
    ["build: set up work folders", /work-folder or branch/],
    ["feat: wave three packages", /process vocabulary/],
    ["feat: WP12 composer", /process vocabulary/],
    ["fix: M5d leftovers", /milestone codes/],
    ["refactor: SSOT for limits", /SSOT/],
    ["feat: folders (work in progress)", /finished work/],
  ];
  for (const [message, why] of cases) {
    assert.match(commitMessageProblems(message).join("\n"), why, message);
  }
  // Ordinary words stay allowed.
  assert.deepEqual(commitMessageProblems("fix(ui): keep dry-run results; waveform icon"), []);
});

test("comments and the scissors part are not part of the message", () => {
  assert.equal(
    messageText(
      "feat: x\n# Claude-Session: comment\n# ------------------------ >8 ------------------------\nWIN1",
    ),
    "feat: x\n\n",
  );
});

test("the hook script exits 1 with the reasons, 0 for a good message", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "nail-msg-"));
  try {
    const file = path.join(dir, "MSG");
    fs.writeFileSync(file, GOOD);
    assert.equal(spawnSync(process.execPath, [SCRIPT, file]).status, 0);
    fs.writeFileSync(file, "Merge work-3 into work-1\n\nClaude-Session: x\n");
    const bad = spawnSync(process.execPath, [SCRIPT, file], { encoding: "utf8" });
    assert.equal(bad.status, 1);
    assert.match(bad.stderr, /commit message: .*work-folder or branch/);
    assert.match(bad.stderr, /commit message: .*session trailer/);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});
