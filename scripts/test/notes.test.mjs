// Tests of scripts/lib/notes.mjs and scripts/check-notes.mjs (ADR 0200).
// Run: node --test "scripts/test/*.test.mjs"
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { NOTES, hasSessionLine, withoutSessionLines } from "../lib/notes.mjs";

const SCRIPT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "check-notes.mjs");

// Built at runtime, so this file holds no such line itself.
const KEY = ["origin", "SessionId"].join("");
const NOTE = `---\nname: x\nmetadata:\n  type: project\n  ${KEY}: 0000-1111\n---\n\nBody\n`;

test("notes are the Markdown files of .claude/memory", () => {
  assert.ok(NOTES.test(".claude/memory/next-steps.md"));
  assert.ok(!NOTES.test("docs/plan.md"));
  assert.ok(!NOTES.test(".claude/memory/sub/x.md"));
});

test("session lines are found and removed, nothing else changes", () => {
  assert.equal(hasSessionLine(NOTE), true);
  const clean = withoutSessionLines(NOTE);
  assert.equal(clean, "---\nname: x\nmetadata:\n  type: project\n---\n\nBody\n");
  assert.equal(hasSessionLine(clean), false);
  assert.equal(withoutSessionLines(clean), clean);
});

test("the check refuses a note with a session line, --fix removes it", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "nail-notes-"));
  try {
    const file = path.join(dir, "note.md");
    fs.writeFileSync(file, NOTE);
    const bad = spawnSync(process.execPath, [SCRIPT, file], { encoding: "utf8" });
    assert.equal(bad.status, 1);
    assert.match(bad.stderr, /session data/);
    assert.equal(spawnSync(process.execPath, [SCRIPT, "--fix", file]).status, 0);
    assert.equal(hasSessionLine(fs.readFileSync(file, "utf8")), false);
    assert.equal(spawnSync(process.execPath, [SCRIPT, file]).status, 0);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});
