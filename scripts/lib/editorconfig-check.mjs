// EditorConfig conformance check (quality baseline Q1, ADR 0123).
//
// Checks each file against the properties `.editorconfig` gives it:
// charset (utf-8: valid UTF-8 without a BOM), end_of_line (lf / crlf),
// insert_final_newline, trim_trailing_whitespace and indent_style (the
// leading whitespace of every line is tabs or spaces only, as configured).
// Indentation width is left to the formatters (rustfmt, Prettier, taplo).
// Files that look binary (a NUL byte in the first 8 KiB) are skipped.
import fs from "node:fs";
import path from "node:path";
import { parseSync } from "editorconfig";

/** @typedef {{ line: number, message: string }} Problem */
/**
 * @typedef {{
 *   charset?: unknown,
 *   end_of_line?: unknown,
 *   insert_final_newline?: unknown,
 *   trim_trailing_whitespace?: unknown,
 *   indent_style?: unknown,
 * }} EditorConfigProps
 */

/** At most this many problems are reported per file and rule. */
const MAX_PER_RULE = 3;

/**
 * Whether the bytes look like a binary file.
 *
 * @param {Uint8Array} bytes
 */
export function isBinary(bytes) {
  return bytes.subarray(0, 8192).includes(0);
}

/**
 * Checks file content against EditorConfig properties.
 *
 * @param {Uint8Array} bytes the file content
 * @param {EditorConfigProps} props the properties for this file
 * @returns {Problem[]}
 */
export function checkContent(bytes, props) {
  /** @type {Problem[]} */
  const problems = [];
  if (bytes.length === 0 || isBinary(bytes)) return problems;

  if (props.charset === "utf-8") {
    if (bytes[0] === 0xef && bytes[1] === 0xbb && bytes[2] === 0xbf) {
      problems.push({ line: 1, message: "byte order mark (charset = utf-8 has none)" });
    }
    try {
      new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    } catch {
      problems.push({ line: 1, message: "not valid UTF-8 (charset = utf-8)" });
      return problems;
    }
  }

  const text = new TextDecoder("utf-8").decode(bytes);
  const lines = text.split("\n");
  const endsWithNewline = text.endsWith("\n");
  if (endsWithNewline) lines.pop();

  /** @type {Record<string, number>} */
  const counts = {};
  /**
   * @param {string} rule
   * @param {number} line
   * @param {string} message
   */
  const report = (rule, line, message) => {
    counts[rule] = (counts[rule] ?? 0) + 1;
    if (counts[rule] <= MAX_PER_RULE) problems.push({ line, message });
  };

  lines.forEach((raw, index) => {
    const line = index + 1;
    const hasCr = raw.endsWith("\r");
    const content = hasCr ? raw.slice(0, -1) : raw;
    const isLast = index === lines.length - 1;
    const terminated = !isLast || endsWithNewline;
    if (props.end_of_line === "lf" && (hasCr || content.includes("\r"))) {
      report("eol", line, "CRLF / CR line ending (end_of_line = lf)");
    }
    if (props.end_of_line === "crlf" && terminated && !hasCr) {
      report("eol", line, "LF line ending (end_of_line = crlf)");
    }
    if (props.trim_trailing_whitespace === true && /[ \t]+$/.test(content)) {
      report("trailing", line, "trailing whitespace (trim_trailing_whitespace = true)");
    }
    const indent = /^[ \t]*/.exec(content)?.[0] ?? "";
    if (props.indent_style === "space" && indent.includes("\t")) {
      report("indent", line, "tab in indentation (indent_style = space)");
    }
    if (props.indent_style === "tab" && /^\t* +\t/.test(indent)) {
      report("indent", line, "space before a tab in indentation (indent_style = tab)");
    }
  });

  if (props.insert_final_newline === true && !endsWithNewline) {
    problems.push({
      line: lines.length,
      message: "no final newline (insert_final_newline = true)",
    });
  }
  if (props.insert_final_newline === false && endsWithNewline) {
    problems.push({ line: lines.length, message: "final newline (insert_final_newline = false)" });
  }
  return problems;
}

/**
 * Checks files on disk; returns `path:line: message` strings.
 *
 * @param {string[]} files paths relative to `root`
 * @param {string} root the repository root
 */
export function checkFiles(files, root) {
  /** @type {Map<string, any>} */
  const cache = new Map();
  /** @type {string[]} */
  const out = [];
  for (const file of files) {
    const absolute = path.resolve(root, file);
    let bytes;
    try {
      bytes = fs.readFileSync(absolute);
    } catch {
      continue; // deleted or a directory (submodule)
    }
    /** @type {EditorConfigProps} */
    const props = parseSync(absolute, { cache, unset: true });
    for (const problem of checkContent(bytes, props)) {
      out.push(`${file.split(path.sep).join("/")}:${String(problem.line)}: ${problem.message}`);
    }
  }
  return out;
}
