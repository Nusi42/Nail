// Tests of scripts/lib/run.mjs. Run: node --test scripts/test/
import assert from "node:assert/strict";
import { test } from "node:test";

import { output, quote } from "../lib/run.mjs";

const PRINT_KEY = "process.stdout.write(process.env.TAURI_SIGNING_PRIVATE_KEY ?? 'none')";

test("a child never sees the update key unless it is given an environment", () => {
  const before = process.env.TAURI_SIGNING_PRIVATE_KEY;
  process.env.TAURI_SIGNING_PRIVATE_KEY = "secret";
  try {
    assert.equal(output(process.execPath, ["-e", PRINT_KEY], { shell: false }), "none");
    const env = { ...process.env };
    assert.equal(output(process.execPath, ["-e", PRINT_KEY], { shell: false, env }), "secret");
  } finally {
    if (before === undefined) delete process.env.TAURI_SIGNING_PRIVATE_KEY;
    else process.env.TAURI_SIGNING_PRIVATE_KEY = before;
  }
});

test("arguments are quoted for cmd.exe only when needed", () => {
  assert.equal(quote("plain"), "plain");
  assert.equal(quote("two words"), '"two words"');
  assert.equal(quote('say "hi"'), '"say ""hi"""');
  assert.equal(quote("a&b"), '"a&b"');
});
