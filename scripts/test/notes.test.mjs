// Tests of scripts/lib/notes.mjs, scripts/check-notes.mjs and the note
// cleaning of scripts/lib/after-edit.mjs (ADR 0200).
// Run: node --test "scripts/test/*.test.mjs"
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { cleanNote } from "../lib/after-edit.mjs";
import {
  NOTES,
  SESSION_KEY,
  TOOL_KEYS,
  hasSessionLine,
  hasToolKeys,
  withoutToolKeys,
} from "../lib/notes.mjs";
import { withTempDir } from "./temp-repo.mjs";

const SCRIPT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "check-notes.mjs");

// The keys come from notes.mjs, so this file holds no such line itself.
const [, MODIFIED, NODE_TYPE] = TOOL_KEYS;
const CLEAN = "---\nname: x\nmetadata:\n  type: project\n---\n\nBody\n";
// Claude Code writes them indented under metadata and at the top level.
const NOTE =
  `---\nname: x\n${String(MODIFIED)}: 2026-10-31T10:00:00Z\nmetadata:\n  type: project\n` +
  `  ${SESSION_KEY}: 0000-1111\n  ${String(NODE_TYPE)}: memory\n---\n\nBody\n`;

test("notes are the Markdown files of .claude/memory", () => {
  assert.ok(NOTES.test(".claude/memory/next-steps.md"));
  assert.ok(!NOTES.test("docs/plan.md"));
  assert.ok(!NOTES.test(".claude/memory/sub/x.md"));
});

test("Claude Code's front-matter keys are found and removed, nothing else changes", () => {
  assert.deepEqual(TOOL_KEYS, [SESSION_KEY, "modified", "node_type"]);
  assert.equal(hasSessionLine(NOTE), true);
  assert.equal(hasToolKeys(NOTE), true);
  assert.equal(withoutToolKeys(NOTE), CLEAN);
  assert.equal(hasToolKeys(CLEAN), false);
  assert.equal(withoutToolKeys(CLEAN), CLEAN);
  // Windows line endings.
  assert.equal(withoutToolKeys(NOTE.replaceAll("\n", "\r\n")), CLEAN.replaceAll("\n", "\r\n"));
  // Each key alone.
  for (const key of TOOL_KEYS) {
    const one = CLEAN.replace("  type: project\n", `  type: project\n  ${key}: v\n`);
    assert.equal(hasToolKeys(one), true, key);
    assert.equal(withoutToolKeys(one), CLEAN, key);
  }
});

test("only the front matter counts: the body may name the keys", () => {
  const body = `${CLEAN}${SESSION_KEY}: quoted in the text\n${String(MODIFIED)}: too\n`;
  assert.equal(hasSessionLine(body), false);
  assert.equal(hasToolKeys(body), false);
  assert.equal(withoutToolKeys(body), body);
  const plain = `${SESSION_KEY}: no front matter\n`;
  assert.equal(withoutToolKeys(plain), plain);
});

test("the check refuses a note with a session line, --fix removes all three keys", () => {
  withTempDir("nail-notes-", (dir) => {
    const file = path.join(dir, "note.md");
    fs.writeFileSync(file, NOTE);
    const bad = spawnSync(process.execPath, [SCRIPT, file], { encoding: "utf8" });
    assert.equal(bad.status, 1);
    assert.match(bad.stderr, /session data/);
    assert.equal(spawnSync(process.execPath, [SCRIPT, "--fix", file]).status, 0);
    assert.equal(fs.readFileSync(file, "utf8"), CLEAN);
    assert.equal(spawnSync(process.execPath, [SCRIPT, file]).status, 0);
  });
});

test("the post-edit hook cleans a note it is handed, and only a note", () => {
  withTempDir("nail-notes-", (dir) => {
    const note = path.join(dir, "note.md");
    fs.writeFileSync(note, NOTE);
    assert.equal(cleanNote(note, ".claude/memory/note.md"), true);
    assert.equal(fs.readFileSync(note, "utf8"), CLEAN);
    assert.equal(cleanNote(note, ".claude/memory/note.md"), false);
    const doc = path.join(dir, "doc.md");
    fs.writeFileSync(doc, NOTE);
    assert.equal(cleanNote(doc, "docs/doc.md"), false);
    assert.equal(fs.readFileSync(doc, "utf8"), NOTE);
  });
});
