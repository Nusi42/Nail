// Tests of scripts/lib/editorconfig-check.mjs (ADR 0123). Run: node --test "scripts/test/*.test.mjs"
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";

import { checkContent, checkFiles, isBinary } from "../lib/editorconfig-check.mjs";

const strict = {
  charset: "utf-8",
  end_of_line: "lf",
  insert_final_newline: true,
  trim_trailing_whitespace: true,
  indent_style: "space",
};

/** @param {string} text */
const bytes = (text) => new TextEncoder().encode(text);

/**
 * @param {string} text
 * @param {Record<string, unknown>} [props]
 */
const messages = (text, props = strict) => checkContent(bytes(text), props).map((p) => p.message);

test("a conforming file has no problems", () => {
  assert.deepEqual(messages("fn main() {\n    run();\n}\n"), []);
  assert.deepEqual(messages(""), [], "empty files are fine");
});

test("CRLF line endings are reported with their line", () => {
  const problems = checkContent(bytes("a\r\nb\n"), strict);
  assert.deepEqual(problems, [{ line: 1, message: "CRLF / CR line ending (end_of_line = lf)" }]);
});

test("end_of_line = crlf wants CRLF on every terminated line", () => {
  assert.deepEqual(messages("a\r\nb\r\n", { end_of_line: "crlf" }), []);
  assert.deepEqual(messages("a\nb\r\n", { end_of_line: "crlf" }), [
    "LF line ending (end_of_line = crlf)",
  ]);
});

test("final newline", () => {
  assert.deepEqual(messages("a"), ["no final newline (insert_final_newline = true)"]);
  assert.deepEqual(messages("a\n", { insert_final_newline: false }), [
    "final newline (insert_final_newline = false)",
  ]);
});

test("trailing whitespace, including before a CR", () => {
  assert.deepEqual(messages("a \nb\t\n"), [
    "trailing whitespace (trim_trailing_whitespace = true)",
    "trailing whitespace (trim_trailing_whitespace = true)",
  ]);
  assert.deepEqual(messages("a \r\n", { trim_trailing_whitespace: true }), [
    "trailing whitespace (trim_trailing_whitespace = true)",
  ]);
});

test("indentation style", () => {
  assert.deepEqual(messages("\tx\n"), ["tab in indentation (indent_style = space)"]);
  assert.deepEqual(messages("x\ty\n"), [], "a tab after the indentation is content");
  assert.deepEqual(messages("\t\tx\n", { indent_style: "tab" }), []);
  assert.deepEqual(messages("  \tx\n", { indent_style: "tab" }), [
    "space before a tab in indentation (indent_style = tab)",
  ]);
});

test("charset utf-8: BOM and invalid bytes", () => {
  const bom = Uint8Array.from([0xef, 0xbb, 0xbf, 0x61, 0x0a]);
  assert.deepEqual(
    checkContent(bom, strict).map((p) => p.message),
    ["byte order mark (charset = utf-8 has none)"],
  );
  const latin1 = Uint8Array.from([0x61, 0xe9, 0x0a]);
  assert.deepEqual(
    checkContent(latin1, strict).map((p) => p.message),
    ["not valid UTF-8 (charset = utf-8)"],
  );
});

test("unset properties check nothing; binary files are skipped", () => {
  assert.deepEqual(messages("a \r\n\tb", {}), []);
  assert.ok(isBinary(Uint8Array.from([0x89, 0x50, 0x00, 0x47])));
  assert.deepEqual(checkContent(Uint8Array.from([0x61, 0x00, 0x20, 0x0d]), strict), []);
});

test("at most three problems per rule and file", () => {
  assert.equal(messages("a \nb \nc \nd \ne \n").length, 3);
});

test("checkFiles reads .editorconfig sections", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "nail-ec-"));
  try {
    fs.writeFileSync(
      path.join(dir, ".editorconfig"),
      "root = true\n[*]\nend_of_line = lf\ninsert_final_newline = true\n[*.eml]\nend_of_line = unset\ninsert_final_newline = unset\n",
    );
    fs.writeFileSync(path.join(dir, "a.txt"), "x\r\n");
    fs.writeFileSync(path.join(dir, "b.eml"), "x\r\ny");
    fs.writeFileSync(path.join(dir, "c.txt"), "ok\n");
    const problems = checkFiles(["a.txt", "b.eml", "c.txt", "missing.txt"], dir);
    assert.deepEqual(problems, ["a.txt:1: CRLF / CR line ending (end_of_line = lf)"]);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});
