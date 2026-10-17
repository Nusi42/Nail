// The private-data guard (ADR 0192): the owner's private details (addresses,
// domains, providers, the Windows user name, ...) must never reach git. The
// patterns live OUTSIDE every repository, in
// %USERPROFILE%\.nail-private\patterns.txt (one regular expression per line,
// `#` starts a comment, matched case-insensitively). Never copy that file or
// any of its values into the repository; tests use synthetic patterns only.
// scripts/check-private.mjs runs the checks; scripts/test/private-patterns.test.mjs
// tests this module.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

/** Environment variable that points the guard at another patterns file (tests, other machines). */
export const PATTERNS_ENV = "NAIL_PRIVATE_PATTERNS";

/** Encoded forms shorter than this are not searched (they match random base64 / hex too often). */
export const MIN_ENCODED_LENGTH = 5;

/**
 * @typedef {object} Matcher
 * @property {RegExp} regex global regular expression
 * @property {number} line line of the patterns file it comes from (1-based)
 * @property {"pattern" | "base64"} kind a pattern of the file, or the base64 form of a literal one
 */

/**
 * @typedef {object} Finding
 * @property {number} line 1-based line in the scanned text
 * @property {number} column 1-based column
 * @property {string} masked the matched text, masked
 * @property {Matcher} matcher
 */

/**
 * Where the patterns file is: $NAIL_PRIVATE_PATTERNS, else
 * %USERPROFILE%\.nail-private\patterns.txt.
 *
 * @param {NodeJS.ProcessEnv} [env]
 * @returns {string}
 */
export function patternsPath(env = process.env) {
  return env[PATTERNS_ENV] ?? path.join(os.homedir(), ".nail-private", "patterns.txt");
}

/**
 * Reads the patterns file; `undefined` when it does not exist (CI, other
 * machines: nothing to check).
 *
 * @param {string} file
 * @returns {Matcher[] | undefined}
 */
export function loadMatchers(file) {
  let text;
  try {
    text = fs.readFileSync(file, "utf8");
  } catch (error) {
    if (/** @type {NodeJS.ErrnoException} */ (error).code === "ENOENT") return undefined;
    throw error;
  }
  return matchersFrom(text);
}

/**
 * The matchers of a patterns file's text: every pattern (case-insensitive),
 * plus the base64 forms of every pattern that is a plain literal.
 *
 * @param {string} text
 * @returns {Matcher[]}
 */
export function matchersFrom(text) {
  /** @type {Matcher[]} */
  const matchers = [];
  const seen = new Set();
  text.split(/\r?\n/).forEach((raw, index) => {
    const source = raw.trim();
    if (source === "" || source.startsWith("#")) return;
    const line = index + 1;
    let regex;
    try {
      regex = new RegExp(source, "gi");
    } catch {
      // Never echo the pattern: it is private.
      throw new Error(`private patterns: line ${String(line)} is not a valid regular expression`);
    }
    matchers.push({ regex, line, kind: "pattern" });
    const literal = literalOf(source);
    if (literal === undefined) return;
    for (const form of base64Forms(literal)) {
      if (seen.has(form)) continue;
      seen.add(form);
      matchers.push({ regex: new RegExp(escapeRegExp(form), "g"), line, kind: "base64" });
    }
  });
  return matchers;
}

/**
 * The literal text a pattern matches when it is a plain string (word
 * boundaries and anchors aside), else `undefined`.
 *
 * @param {string} source
 * @returns {string | undefined}
 */
export function literalOf(source) {
  const stripped = source.replace(/\\b/g, "").replace(/^\^/, "").replace(/\$$/, "");
  let literal = "";
  for (let i = 0; i < stripped.length; i++) {
    const ch = stripped.charAt(i);
    if (ch === "\\") {
      const next = stripped.charAt(i + 1);
      // \d, \w, \s, ... are classes, not literals.
      if (next === "" || /[A-Za-z0-9]/.test(next)) return undefined;
      literal += next;
      i++;
    } else if (".*+?()[]{}|^$".includes(ch)) {
      return undefined;
    } else {
      literal += ch;
    }
  }
  return literal === "" ? undefined : literal;
}

/**
 * The base64 forms of a literal at the three byte alignments: the characters
 * that depend only on the literal's bytes (so they appear in any base64 text
 * that encodes it), standard and URL-safe alphabets. Forms shorter than
 * MIN_ENCODED_LENGTH are dropped.
 *
 * @param {string} literal
 * @returns {string[]}
 */
export function base64Forms(literal) {
  const bytes = Buffer.from(literal, "utf8");
  /** @type {string[]} */
  const forms = [];
  for (let offset = 0; offset < 3; offset++) {
    const encoded = Buffer.concat([Buffer.alloc(offset), bytes]).toString("base64");
    const start = Math.ceil((8 * offset) / 6);
    const end = Math.floor((8 * (offset + bytes.length)) / 6);
    const form = encoded.slice(start, end);
    if (form.length < MIN_ENCODED_LENGTH) continue;
    forms.push(form);
    const urlSafe = form.replace(/\+/g, "-").replace(/\//g, "_");
    if (urlSafe !== form) forms.push(urlSafe);
  }
  return forms;
}

/**
 * Every match in a text, with line and column and the match masked.
 *
 * @param {string} text
 * @param {Matcher[]} matchers
 * @returns {Finding[]}
 */
export function scanText(text, matchers) {
  /** @type {Finding[]} */
  const findings = [];
  /** @type {number[] | undefined} */
  let starts;
  for (const matcher of matchers) {
    matcher.regex.lastIndex = 0;
    for (const match of text.matchAll(matcher.regex)) {
      if (match[0] === "") continue;
      starts ??= lineStarts(text);
      const at = match.index;
      const line = lineOf(starts, at);
      findings.push({
        line: line + 1,
        column: at - (starts[line] ?? 0) + 1,
        masked: mask(match[0]),
        matcher,
      });
    }
  }
  return findings.sort((a, b) => a.line - b.line || a.column - b.column);
}

/**
 * A matched text for error output: its first character and its length, never the text.
 *
 * @param {string} text
 * @returns {string}
 */
export function mask(text) {
  return `${text.charAt(0)}${"*".repeat(Math.max(0, text.length - 1))}`;
}

/**
 * A text (a file name, a ref) with every match masked, for error output.
 *
 * @param {string} text
 * @param {Matcher[]} matchers
 * @returns {string}
 */
export function maskAll(text, matchers) {
  let out = text;
  for (const matcher of matchers) {
    matcher.regex.lastIndex = 0;
    out = out.replace(matcher.regex, (m) => mask(m));
  }
  return out;
}

/**
 * One error line: where, which pattern line, the masked match.
 *
 * @param {string} where file name or `commit <id> message` etc.
 * @param {Finding} finding
 * @returns {string}
 */
export function describe(where, finding) {
  const what =
    finding.matcher.kind === "base64"
      ? `the base64 form of private pattern ${String(finding.matcher.line)}`
      : `private pattern ${String(finding.matcher.line)}`;
  return `${where}:${String(finding.line)}:${String(finding.column)}: ${what} matched "${finding.masked}"`;
}

/**
 * The `git log` format `pushSections` reads: the commit id, the author and
 * the committer (name and address), then the message.
 */
export const PUSH_LOG_FORMAT = "--format=%x00commit %H%n%an <%ae>%n%cn <%ce>%n%B";

/**
 * The added lines of a `git log -p` text (`PUSH_LOG_FORMAT`), grouped by
 * file, plus each commit's author, committer, message and changed file names.
 * Removed lines are skipped: a commit that deletes private text is a fix.
 *
 * @param {string} log
 * @returns {{ where: string, text: string }[]}
 */
export function pushSections(log) {
  /** @type {{ where: string, text: string }[]} */
  const sections = [];
  for (const chunk of log.split("\0commit ").filter((c) => c.trim() !== "")) {
    const newline = chunk.indexOf("\n");
    const id = (newline < 0 ? chunk : chunk.slice(0, newline)).trim().replace(/^commit /, "");
    const [author = "", committer = "", ...body] = (
      newline < 0 ? "" : chunk.slice(newline + 1)
    ).split("\n");
    sections.push({ where: `commit ${id.slice(0, 12)} author`, text: author });
    sections.push({ where: `commit ${id.slice(0, 12)} committer`, text: committer });
    const rest = body.join("\n");
    const diffAt = rest.search(/^diff --git /m);
    const message = diffAt < 0 ? rest : rest.slice(0, diffAt);
    sections.push({ where: `commit ${id.slice(0, 12)} message`, text: message });
    if (diffAt < 0) continue;
    for (const fileDiff of rest.slice(diffAt).split(/^(?=diff --git )/m)) {
      const name =
        /^\+\+\+ b\/(.*)$/m.exec(fileDiff)?.[1] ??
        /^diff --git a\/.* b\/(.*)$/m.exec(fileDiff)?.[1];
      const where = `commit ${id.slice(0, 12)} ${name ?? "?"}`;
      if (name !== undefined) sections.push({ where: `${where} (file name)`, text: name });
      const added = fileDiff
        .split("\n")
        .map((l) => (l.startsWith("+") && !l.startsWith("+++ ") ? l.slice(1) : ""))
        .join("\n");
      sections.push({ where: `${where} (added lines; line = line of the diff)`, text: added });
    }
  }
  return sections;
}

/** @param {string} text */
function lineStarts(text) {
  const starts = [0];
  for (let i = text.indexOf("\n"); i >= 0; i = text.indexOf("\n", i + 1)) starts.push(i + 1);
  return starts;
}

/**
 * @param {number[]} starts
 * @param {number} at
 */
function lineOf(starts, at) {
  let low = 0;
  let high = starts.length - 1;
  while (low < high) {
    const mid = Math.ceil((low + high) / 2);
    if ((starts[mid] ?? 0) <= at) low = mid;
    else high = mid - 1;
  }
  return low;
}

/** @param {string} text */
function escapeRegExp(text) {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
