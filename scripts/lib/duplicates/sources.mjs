// The repository's source files as the SSOT checks see them, and a small
// lexer for Rust / TypeScript / JavaScript (comments, strings, template
// literals). Tests in scripts/test/duplicates.test.mjs.
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

import { IGNORES } from "./config.mjs";

/** @typedef {{ path: string, text: string }} SourceFile */

/** Code files the SSOT checks read. */
export const CODE = /\.(rs|ts|tsx|mts|cts|js|mjs|cjs)$/;

/**
 * Whether a glob of {@link IGNORES} matches `file`. Supports `**`, `*` and
 * literal text, which is all the list uses.
 *
 * @param {string} glob
 * @param {string} file
 */
export function globMatches(glob, file) {
  let pattern = "";
  for (let i = 0; i < glob.length; i++) {
    const c = glob[i] ?? "";
    if (c === "*" && glob[i + 1] === "*") {
      // `**/` = any number of directories (also none); a trailing `**` = anything.
      if (glob[i + 2] === "/") {
        pattern += "(?:.*/)?";
        i += 2;
      } else {
        pattern += ".*";
        i += 1;
      }
    } else if (c === "*") pattern += "[^/]*";
    else pattern += c.replace(/[.+?^${}()|[\]\\]/g, "\\$&");
  }
  return new RegExp(`^${pattern}$`).test(file);
}

/**
 * Whether `file` is one the checks skip (generated code, fixtures, ...).
 *
 * @param {string} file
 */
export function isIgnored(file) {
  return IGNORES.some((i) => globMatches(i.glob, file));
}

/**
 * Whether `file` is test code (tests, test helpers, mock servers, the UI's
 * mock core and seeds). The duplicate check scans tests too; only the
 * magic-number and cross-language constant checks skip them.
 *
 * @param {string} file
 */
export function isTestPath(file) {
  return (
    /(^|\/)(tests?|__tests__|testing|test-support)(\/|$)/.test(file) ||
    /\.(test|spec|perf)\.[cm]?[jt]sx?$/.test(file) ||
    /(^|\/|_)tests?\.rs$/.test(file) ||
    file.startsWith("e2e/") ||
    file.startsWith("crates/nail-testkit") ||
    /(^|\/)mock[A-Z\w]*\.tsx?$|[Mm]ock\.tsx?$|mockSeed\.ts$|Seed\.ts$/.test(file)
  );
}

/**
 * Whether `file` is part of the UI's mock core (the stand-in for the core in
 * Vitest and `?mock=1`: `mock*.ts`, `*Mock.ts`, seeds). It counts as test
 * code for the magic-number check, but its contract limits and lists are
 * checked like production code: they must come from the generated
 * constants, not be copied.
 *
 * @param {string} file
 */
export function isMockCorePath(file) {
  return (
    file.startsWith("ui/src/") &&
    !/\.(test|spec)\.[cm]?[jt]sx?$/.test(file) &&
    !/(^|\/)(tests?|__tests__)\//.test(file) &&
    /(^|\/)mock[A-Z\w]*\.tsx?$|[Mm]ock\.tsx?$|mockSeed\.ts$|Seed\.ts$/.test(file)
  );
}

/**
 * The files of the checkout below `root` (`/` separators): tracked and
 * new ones, without git-ignored ones and without {@link IGNORES}.
 *
 * @param {string} root
 * @returns {string[]}
 */
export function trackedFiles(root) {
  // Tracked and new (untracked, not ignored) files: a file an agent just
  // created is checked before it is staged.
  return execFileSync("git", ["ls-files", "-z", "--cached", "--others", "--exclude-standard"], {
    cwd: root,
    encoding: "utf8",
    maxBuffer: 1 << 28,
  })
    .split("\0")
    .filter((f) => f !== "" && !isIgnored(f));
}

/**
 * Reads the tracked files matching `pattern` (working-tree copies; files
 * deleted but not yet staged are skipped).
 *
 * @param {string} root
 * @param {RegExp} pattern
 * @returns {SourceFile[]}
 */
export function readSources(root, pattern) {
  /** @type {SourceFile[]} */
  const out = [];
  for (const file of trackedFiles(root)) {
    if (!pattern.test(file)) continue;
    const full = path.join(root, file);
    if (!fs.existsSync(full)) continue;
    out.push({ path: file, text: fs.readFileSync(full, "utf8") });
  }
  return out;
}

/**
 * @typedef {object} Segment
 * @property {"code" | "comment" | "string" | "template"} kind
 * @property {string} text the segment's text (for strings and templates
 *   without the quotes; template interpolations are replaced by `ph`)
 * @property {number} line 1-based line where it starts
 */

/**
 * Splits Rust / TS / JS source into code, comments, strings and template
 * literals. Good enough for these checks: regex literals are read as code,
 * Rust raw strings (`r#"..."#`) and char literals are handled, lifetimes
 * (`'a`) are code. Template interpolations become `ph` in the template's
 * text; the code inside them is not returned.
 *
 * @param {string} text
 * @param {{ rust?: boolean, ph?: string }} [options]
 * @returns {Segment[]}
 */
export function lex(text, options = {}) {
  const rust = options.rust ?? false;
  const ph = options.ph ?? "_";
  /** @type {Segment[]} */
  const out = [];
  let line = 1;
  let i = 0;
  let codeStart = 0;
  let codeLine = 1;
  /** @param {number} end */
  const flushCode = (end) => {
    if (end > codeStart)
      out.push({ kind: "code", text: text.slice(codeStart, end), line: codeLine });
  };
  /** @param {number} from @param {number} to */
  const countLines = (from, to) => {
    for (let k = from; k < to; k++) if (text[k] === "\n") line++;
  };
  while (i < text.length) {
    const c = text[i];
    const next = text[i + 1];
    let end = -1;
    /** @type {Segment["kind"] | null} */
    let kind = null;
    let body = "";
    const startLine = line;
    if (c === "/" && next === "/") {
      end = text.indexOf("\n", i);
      if (end === -1) end = text.length;
      kind = "comment";
      body = text.slice(i + 2, end);
    } else if (c === "/" && next === "*") {
      end = text.indexOf("*/", i + 2);
      end = end === -1 ? text.length : end + 2;
      kind = "comment";
      body = text.slice(i + 2, end - 2);
    } else if (
      rust &&
      c === "r" &&
      (next === "#" || next === '"') &&
      !/\w/.test(text[i - 1] ?? "")
    ) {
      const m = /^r(#*)"/.exec(text.slice(i, i + 20));
      if (m) {
        const close = `"${m[1] ?? ""}`;
        const from = i + m[0].length;
        const to = text.indexOf(close, from);
        end = to === -1 ? text.length : to + close.length;
        kind = "string";
        body = text.slice(from, to === -1 ? text.length : to);
      }
    } else if (c === '"' || (!rust && c === "'")) {
      let k = i + 1;
      while (k < text.length && text[k] !== c && text[k] !== "\n") k += text[k] === "\\" ? 2 : 1;
      if (rust && c === '"') {
        // Rust strings may span lines.
        k = i + 1;
        while (k < text.length && text[k] !== '"') k += text[k] === "\\" ? 2 : 1;
      }
      end = Math.min(k + 1, text.length);
      kind = "string";
      body = text.slice(i + 1, end - 1);
    } else if (rust && c === "'") {
      // A char literal ('a', '\n', '\u{1F600}'), else a lifetime / label.
      const m = /^'(?:\\(?:u\{[0-9a-fA-F]+\}|.)|[^\\'])'/.exec(text.slice(i, i + 12));
      if (m) {
        end = i + m[0].length;
        kind = "string";
        body = m[0].slice(1, -1);
      }
    } else if (!rust && c === "`") {
      const parsed = readTemplate(text, i, ph);
      end = parsed.end;
      kind = "template";
      body = parsed.text;
    }
    if (kind === null) {
      if (c === "\n") line++;
      i++;
      continue;
    }
    flushCode(i);
    out.push({ kind, text: body, line: startLine });
    countLines(i, end);
    i = end;
    codeStart = end;
    codeLine = line;
  }
  flushCode(text.length);
  return out;
}

/**
 * Reads a template literal starting at the backtick `start`.
 *
 * @param {string} text
 * @param {number} start
 * @param {string} ph replacement for each `${...}`
 * @returns {{ end: number, text: string }}
 */
function readTemplate(text, start, ph) {
  let out = "";
  let i = start + 1;
  while (i < text.length) {
    const c = text[i] ?? "";
    if (c === "\\") {
      out += text.slice(i, i + 2);
      i += 2;
    } else if (c === "`") {
      return { end: i + 1, text: out };
    } else if (c === "$" && text[i + 1] === "{") {
      i = skipInterpolation(text, i + 2);
      out += ph;
    } else {
      out += c;
      i++;
    }
  }
  return { end: text.length, text: out };
}

/**
 * Skips the code of an interpolation (nested braces, strings, templates);
 * returns the index after its closing brace.
 *
 * @param {string} text
 * @param {number} i
 */
function skipInterpolation(text, i) {
  let depth = 1;
  while (i < text.length) {
    const c = text[i];
    if (c === "`") i = readTemplate(text, i, "").end;
    else if (c === '"' || c === "'") {
      let k = i + 1;
      while (k < text.length && text[k] !== c && text[k] !== "\n") k += text[k] === "\\" ? 2 : 1;
      i = k + 1;
    } else {
      if (c === "{") depth++;
      else if (c === "}" && --depth === 0) return i + 1;
      i++;
    }
  }
  return i;
}

/**
 * Rust source without its `#[cfg(test)]` modules (from the first one to the
 * end of the file: test modules sit at the end in this repository).
 *
 * @param {string} text
 */
export function withoutRustTests(text) {
  const cut = text.search(/^\s*#\[cfg\(test\)\]\s*\n\s*(pub(\([a-z]+\))?\s+)?mod\b/m);
  return cut === -1 ? text : text.slice(0, cut);
}
