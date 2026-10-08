// The SSOT / ODR checks a token-based clone detector cannot see (wave Q2,
// ADR 0171): hand mirrors between Rust and TypeScript, one constant defined
// in several places, one English text under several Fluent ids, theme values
// hard-coded in CSS-in-TS, and magic numbers repeated across files. Every
// detector is a pure function of the files it is given; tests in
// scripts/test/duplicates.test.mjs.
import { COMMON_NUMBERS, MAGIC_NUMBER_MIN, MAGIC_NUMBER_MIN_FILES, SELF } from "./config.mjs";
import { ownerOfPath } from "./owners.mjs";
import { isTestPath, lex, withoutRustTests } from "./sources.mjs";

/**
 * @typedef {import("./sources.mjs").SourceFile} SourceFile
 *
 * @typedef {object} Finding
 * @property {string} check the detector's id
 * @property {string} key stable identity (no line numbers) for baselines and exceptions
 * @property {string[]} files the files involved (`/` separators)
 * @property {string} detail what was found, with places (`file:line`)
 * @property {boolean} [mirror] a Rust <-> TypeScript mirror (owned by WP-mirrors-tooling)
 * @property {string[]} [members] what the finding spans when that is not its
 *   files (the Fluent ids of one text); part of its baseline identity
 * @property {string} [group] clones of one copied fragment share it (jscpd
 *   counts a fragment's copies, so a third copy may be reported on the pair
 *   of the first two)
 */

/**
 * A finding's identity in the ratchet baseline: its key plus what it spans
 * (members, else files), so a finding that grows (a third copy) counts as
 * new and fails ({@link knownInBaseline}).
 *
 * @param {Finding} finding
 */
export function baselineIdentity(finding) {
  const members = finding.members ?? finding.files;
  return members.length > 1 ? `${finding.key} @ ${members.join(", ")}` : finding.key;
}

/**
 * Whether the baseline knows `finding`: its identity is recorded, or a
 * recorded finding of the same key spans every member it spans (a finding
 * that only lost members — one copy of three removed, often by another
 * package's fix — is progress, not a regression). A finding with a member
 * the baseline does not list is new.
 *
 * @param {Finding} finding
 * @param {readonly string[]} recorded the baseline identities of its check
 */
export function knownInBaseline(finding, recorded) {
  const identity = baselineIdentity(finding);
  if (recorded.includes(identity)) return true;
  const members = finding.members ?? finding.files;
  const prefix = `${finding.key} @ `;
  return recorded.some((entry) => {
    if (!entry.startsWith(prefix)) return false;
    const listed = new Set(entry.slice(prefix.length).split(", "));
    return members.every((m) => listed.has(m));
  });
}

/** The SSOT checks, in report order, with what each finds. */
export const SSOT_CHECKS = {
  "mirror-comment":
    "a comment says code mirrors / is kept in sync with another source (Rust <-> TypeScript, prelude.js, theme.kdl): generate it, or pin it with a test named in the comment (`pinned by <test file>`)",
  "mirror-inventory":
    "a known hand mirror listed in scripts/duplicates/mirrors.json (found by reading, not by a detector): generate it or pin it with a test, then remove the entry",
  "rust-ts-list":
    "the same list of string literals in Rust and in TypeScript / JavaScript: generate the TypeScript side or pin it with a test",
  "rust-ts-const":
    "a constant of the same name defined in Rust and in TypeScript / JavaScript: generate the TypeScript side (gen:api) or pin it with a test",
  "const-value":
    "the same constant (name and value) defined in several files of one language: define it once and import it",
  "fluent-text":
    "the same English text under several Fluent ids: one id per meaning (exceptions for different meanings in allow.json)",
  "theme-literal": "a colour literal in CSS-in-TS: use the theme's token (`var(--nail-color-…)`)",
  "css-length":
    "the same CSS length in several skin modules or in a skin and defaults/*.kdl: one token or one shared constant",
  "magic-number":
    "the same number in several non-test source files: one named constant (exceptions for unrelated meanings in allow.json)",
};

/** @typedef {keyof typeof SSOT_CHECKS} SsotCheck */

const MIRROR_WORD =
  /\bmirror(s|ed|ing)?\b|\bby hand\b|\bkeep(s)? in sync\b|\bkept in sync\b|\bderives? the same\b|\bsame rule as\b|\bin step with\b/i;
/** In TypeScript / JavaScript: the comment names a Rust or configuration source. */
const RUST_SIDE =
  /nail_api|\bRust\b|\.rs\b|\bcontract'?s?\b|prelude\.js|theme\.kdl|layout\.kdl|`[a-z_]+::[A-Za-z_:]+`/;
/** In Rust: the comment names a TypeScript / UI source. */
const TS_SIDE = /\bui\/|\.tsx?\b|TypeScript|\bthe UI'?s?\b|prelude\.js|plugin-api/;
const PINNED = /pinned by `([^`]+)`/;

/**
 * `mirror-comment`: comments that announce a hand mirror. A comment that
 * names its pinning test (`pinned by \`path\``, an existing file) is fine.
 *
 * @param {SourceFile[]} files code files
 * @param {(file: string) => boolean} exists whether a repository path exists
 * @returns {Finding[]}
 */
export function mirrorComments(files, exists) {
  /** @type {Finding[]} */
  const out = [];
  for (const file of files) {
    if (SELF.test(file.path)) continue;
    const rust = file.path.endsWith(".rs");
    for (const seg of lex(file.text, { rust })) {
      if (seg.kind !== "comment") continue;
      // Doc comments span several `//` segments; look at each line.
      const lines = seg.text.split("\n");
      lines.forEach((raw, n) => {
        const text = raw.replace(/^[\s*/!]+/, "").trim();
        if (!MIRROR_WORD.test(text)) return;
        // The other source may be named on the next line of the comment.
        const context = `${text} ${lines[n + 1] ?? ""}`;
        if (!(rust ? TS_SIDE : RUST_SIDE).test(context)) return;
        const pinned = PINNED.exec(context)?.[1];
        if (pinned !== undefined && exists(pinned)) return;
        out.push({
          check: "mirror-comment",
          key: `${file.path}: ${text}`,
          files: [file.path],
          detail: `${file.path}:${String(seg.line + n)}: ${text}`,
          mirror: true,
        });
      });
    }
  }
  return dedupe(out);
}

/**
 * @typedef {object} ConstDef
 * @property {string} file
 * @property {number} line
 * @property {string} name
 * @property {string} value normalized single-line value ("" when it spans lines)
 * @property {string[]} literals the string and number literals of the whole
 *   definition (up to its `;`), for telling two facts of one name apart
 */

const RUST_CONST =
  /^[ \t]*(?:pub(?:\([a-z]+\))?\s+)?(?:const|static)\s+([A-Z][A-Z0-9_]*)\s*:[^=\n]*=\s*([^\n]*)$/gm;
const TS_CONST = /^[ \t]*(?:export\s+)?const\s+([A-Z][A-Z0-9_]*)\s*(?::[^=\n]*)?=\s*([^\n]*)$/gm;

/**
 * Normalizes a constant's value for comparison: whitespace collapsed, digit
 * separators dropped, `as const` / trailing `;` removed; a value that
 * continues on the next line is "" (not compared).
 *
 * @param {string} raw
 */
export function normalizeValue(raw) {
  const value = raw
    .replace(/\/\/.*$/, "")
    .trim()
    .replace(/;$/, "")
    .replace(/\s+as\s+const$/, "")
    .replace(/(\d)_(?=\d)/g, "$1")
    .replace(/\s+/g, " ")
    .trim();
  const open = (value.match(/[[({]/g) ?? []).length;
  const close = (value.match(/[\])}]/g) ?? []).length;
  if (value === "" || open !== close || /[=,+(]$/.test(value)) return "";
  return value;
}

/**
 * Constant definitions (`UPPER_SNAKE` names) of a file.
 *
 * @param {SourceFile} file
 * @param {{ skipTests?: boolean }} [options] Rust: drop `#[cfg(test)]` modules
 * @returns {ConstDef[]}
 */
export function constDefs(file, options = {}) {
  const rust = file.path.endsWith(".rs");
  const text = rust && options.skipTests === true ? withoutRustTests(file.text) : file.text;
  /** @type {ConstDef[]} */
  const out = [];
  for (const m of text.matchAll(rust ? RUST_CONST : TS_CONST)) {
    const line = text.slice(0, m.index).split("\n").length;
    out.push({
      file: file.path,
      line,
      name: m[1] ?? "",
      value: normalizeValue(m[2] ?? ""),
      literals: literalsOf(statementAt(text, m.index + m[0].length - (m[2] ?? "").length), rust),
    });
  }
  return out;
}

/**
 * The text of a definition from `start` to its `;` (outside brackets and
 * strings), at most 20,000 characters.
 *
 * @param {string} text
 * @param {number} start
 */
export function statementAt(text, start) {
  let depth = 0;
  /** @type {string | null} */
  let quote = null;
  const end = Math.min(text.length, start + 20_000);
  for (let i = start; i < end; i++) {
    const c = text[i];
    if (quote !== null) {
      if (c === "\\") i++;
      else if (c === quote) quote = null;
    } else if (c === '"' || c === "`" || (c === "'" && /\W/.test(text[i - 1] ?? " "))) quote = c;
    else if (c === "(" || c === "[" || c === "{") depth++;
    else if (c === ")" || c === "]" || c === "}") depth--;
    else if (c === ";" && depth <= 0) return text.slice(start, i);
  }
  return text.slice(start, end);
}

/**
 * The string and number literals of some code (numbers without digit
 * separators), sorted and unique.
 *
 * @param {string} code
 * @param {boolean} rust
 * @returns {string[]}
 */
export function literalsOf(code, rust) {
  /** @type {Set<string>} */
  const out = new Set();
  for (const seg of lex(code, { rust })) {
    if (seg.kind === "string" || seg.kind === "template") out.add(JSON.stringify(seg.text));
    else if (seg.kind === "code") {
      for (const m of seg.text.matchAll(NUMBER)) out.add((m[1] ?? "").replace(/_/g, ""));
    }
  }
  return [...out].sort();
}

/**
 * `rust-ts-const`: a constant name defined in non-test Rust code and in
 * non-test TypeScript / JavaScript, unless the two definitions share no
 * literal at all (then they are two facts that happen to share a name, like
 * `LISTS` of a corpus and of a skin).
 *
 * @param {SourceFile[]} files code files
 * @returns {Finding[]}
 */
export function rustTsConstants(files) {
  /** @type {Map<string, ConstDef[]>} */
  const rust = new Map();
  /** @type {Map<string, ConstDef[]>} */
  const ts = new Map();
  for (const file of files) {
    if (isTestPath(file.path)) continue;
    const target = file.path.endsWith(".rs") ? rust : ts;
    for (const def of constDefs(file, { skipTests: true })) push(target, def.name, def);
  }
  /** @type {Finding[]} */
  const out = [];
  for (const [name, defs] of rust) {
    const tsDefs = ts.get(name);
    if (tsDefs === undefined) continue;
    const rustLiterals = new Set(defs.flatMap((d) => d.literals));
    const tsLiterals = tsDefs.flatMap((d) => d.literals);
    const related =
      rustLiterals.size === 0 ||
      tsLiterals.length === 0 ||
      tsLiterals.some((l) => rustLiterals.has(l));
    if (!related) continue;
    const all = [...defs, ...tsDefs];
    out.push({
      check: "rust-ts-const",
      key: name,
      files: unique(all.map((d) => d.file)),
      detail: `${name}: ${all.map(place).join(", ")}`,
      mirror: true,
    });
  }
  return out;
}

/** A bracketed list of at least three string literals (one line or several). */
const STRING_LIST = /\[\s*(?:"(?:[^"\\\n]|\\.)*"\s*,\s*){2,}"(?:[^"\\\n]|\\.)*"\s*,?\s*\]/g;

/**
 * The non-test source files with the code the checks scan (Rust without
 * its `#[cfg(test)]` modules).
 *
 * @param {SourceFile[]} files
 * @returns {{ file: SourceFile, rust: boolean, text: string }[]}
 */
export function productionCode(files) {
  return files
    .filter((file) => !isTestPath(file.path))
    .map((file) => {
      const rust = file.path.endsWith(".rs");
      return { file, rust, text: rust ? withoutRustTests(file.text) : file.text };
    });
}

/**
 * `rust-ts-list`: the same set of string literals as a list in non-test
 * Rust code and in non-test TypeScript / JavaScript (enum value lists,
 * colour tables, id lists copied by hand).
 *
 * @param {SourceFile[]} files code files
 * @returns {Finding[]}
 */
export function rustTsLists(files) {
  /** @type {Map<string, { rust: string[], ts: string[] }>} */
  const lists = new Map();
  for (const { file, rust, text } of productionCode(files)) {
    for (const m of text.matchAll(STRING_LIST)) {
      const items = [...m[0].matchAll(/"((?:[^"\\\n]|\\.)*)"/g)].map((i) => i[1] ?? "");
      const key = [...new Set(items)].sort().join(", ");
      const line = text.slice(0, m.index).split("\n").length;
      const entry = lists.get(key) ?? { rust: [], ts: [] };
      (rust ? entry.rust : entry.ts).push(`${file.path}:${String(line)}`);
      lists.set(key, entry);
    }
  }
  /** @type {Finding[]} */
  const out = [];
  for (const [key, at] of lists) {
    if (at.rust.length === 0 || at.ts.length === 0) continue;
    const places = [...at.rust, ...at.ts];
    out.push({
      check: "rust-ts-list",
      key,
      files: unique(places.map((p) => p.replace(/:\d+$/, ""))),
      detail: `[${key.length > 80 ? `${key.slice(0, 80)}…` : key}]: ${places.join(", ")}`,
      mirror: true,
    });
  }
  return out;
}

/**
 * @typedef {object} InventoryEntry
 * @property {string} key short stable name
 * @property {string[]} files the copies (repository paths)
 * @property {string} what what is mirrored, and how to fix it
 * @property {boolean} [mirror] false: a copy inside one package (owned by
 *   the files' package), not a Rust <-> TypeScript mirror (default true)
 */

/**
 * `mirror-inventory`: the known hand mirrors no detector sees (functions
 * ported by hand, limits named differently on each side), listed in
 * scripts/duplicates/mirrors.json. Each entry is a finding until the mirror
 * is generated or pinned and the entry is removed.
 *
 * @param {InventoryEntry[]} entries
 * @param {(file: string) => boolean} exists
 * @returns {{ findings: Finding[], problems: string[] }}
 */
export function mirrorInventory(entries, exists) {
  const problems = entries.flatMap((e) =>
    e.files.filter((f) => !exists(f)).map((f) => `mirrors.json: ${e.key}: ${f} does not exist`),
  );
  const findings = entries.map((e) => ({
    check: "mirror-inventory",
    key: e.key,
    files: [...e.files].sort(),
    detail: `${e.key}: ${e.what} (${e.files.join(", ")})`,
    mirror: e.mirror ?? true,
  }));
  return { findings, problems };
}

/**
 * `const-value`: the same name with the same value in several files of one
 * language (tests included: a test constant copied into every test file is
 * one fact too).
 *
 * @param {SourceFile[]} files code files
 * @returns {Finding[]}
 */
export function sameConstants(files) {
  /** @type {Map<string, ConstDef[]>} */
  const groups = new Map();
  for (const file of files) {
    const lang = file.path.endsWith(".rs") ? "rs" : "ts";
    for (const def of constDefs(file)) {
      if (def.value === "") continue;
      push(groups, `${lang} ${def.name} = ${def.value}`, def);
    }
  }
  /** @type {Finding[]} */
  const out = [];
  for (const [key, defs] of groups) {
    const fileSet = unique(defs.map((d) => d.file));
    if (fileSet.length < 2) continue;
    out.push({
      check: "const-value",
      key,
      files: fileSet,
      detail: `${key.slice(3)}: ${defs.map(place).join(", ")}`,
    });
  }
  return out;
}

/**
 * @typedef {object} FluentEntry
 * @property {string} file
 * @property {number} line
 * @property {string} id message id, or `id.attribute`
 * @property {string} text normalized English text
 */

/**
 * The messages and attributes of a Fluent catalog (multi-line values joined
 * with a space; comments, terms and blank lines skipped).
 *
 * @param {SourceFile} file
 * @returns {FluentEntry[]}
 */
export function fluentEntries(file) {
  /** @type {FluentEntry[]} */
  const out = [];
  /** @type {FluentEntry | undefined} */
  let current;
  let message = "";
  const lines = file.text.split(/\r?\n/);
  lines.forEach((line, n) => {
    const head = /^([a-zA-Z][\w-]*)\s*=\s?(.*)$/.exec(line);
    const attr = /^\s+\.([a-zA-Z][\w-]*)\s*=\s?(.*)$/.exec(line);
    if (head) {
      message = head[1] ?? "";
      current = { file: file.path, line: n + 1, id: message, text: head[2] ?? "" };
      out.push(current);
    } else if (attr && message !== "") {
      current = {
        file: file.path,
        line: n + 1,
        id: `${message}.${attr[1] ?? ""}`,
        text: attr[2] ?? "",
      };
      out.push(current);
    } else if (/^\s+\S/.test(line) && current !== undefined && !line.trimStart().startsWith("#")) {
      current.text += ` ${line.trim()}`;
    } else if (!/^\s/.test(line)) {
      // A comment, a term (-term = ...) or a blank line ends the message.
      if (line.trim() !== "") {
        message = "";
        current = undefined;
      }
    }
  });
  for (const e of out) e.text = e.text.replace(/\s+/g, " ").trim();
  return out.filter((e) => e.text !== "" && !/^\{[^}]*\}$/.test(e.text));
}

/**
 * `fluent-text`: the same English text under several ids. Plural / select
 * variants of one message are one entry; a message's attributes count.
 *
 * @param {SourceFile[]} catalogs `locales/en/*.ftl`
 * @returns {Finding[]}
 */
export function fluentDuplicates(catalogs) {
  /** @type {Map<string, FluentEntry[]>} */
  const byText = new Map();
  for (const file of catalogs) for (const e of fluentEntries(file)) push(byText, e.text, e);
  /** @type {Finding[]} */
  const out = [];
  for (const [text, entries] of byText) {
    if (entries.length < 2) continue;
    out.push({
      check: "fluent-text",
      key: text,
      files: unique(entries.map((e) => e.file)),
      members: unique(entries.map((e) => e.id)),
      detail: `"${text}": ${entries.map((e) => `${e.id} (${e.file}:${String(e.line)})`).join(", ")}`,
    });
  }
  return out;
}

/**
 * @typedef {object} CssBlock
 * @property {string} file
 * @property {number} line 1-based line of the template literal
 * @property {string} css the literal's text (interpolations replaced)
 */

/** Template literals that look like CSS rules. */
const CSS_LIKE = /[{][^{}]*:[^{}]*;[^{}]*[}]/;

/**
 * The CSS-in-TS of the UI: template literals with CSS rules in non-test UI
 * modules.
 *
 * @param {SourceFile[]} files
 * @returns {CssBlock[]}
 */
export function cssBlocks(files) {
  /** @type {CssBlock[]} */
  const out = [];
  for (const file of files) {
    if (!/^ui\/src\/.*\.tsx?$/.test(file.path) || isTestPath(file.path)) continue;
    for (const seg of lex(file.text, { ph: "var(--nail-x)" })) {
      if (seg.kind === "template" && CSS_LIKE.test(seg.text)) {
        out.push({ file: file.path, line: seg.line, css: seg.text });
      }
    }
  }
  return out;
}

/**
 * The CSS-in-TS as one masked text per file, with the same line numbers as
 * the source (everything outside the CSS literals is blank), for jscpd's CSS
 * tokenizer.
 *
 * @param {SourceFile[]} files
 * @returns {Map<string, string>} file -> masked text
 */
export function maskedCss(files) {
  /** @type {Map<string, string[]>} */
  const lines = new Map();
  for (const block of cssBlocks(files)) {
    const source = files.find((f) => f.path === block.file);
    if (source === undefined) continue;
    const out = lines.get(block.file) ?? source.text.split("\n").map(() => "");
    block.css.split("\n").forEach((text, n) => {
      const at = block.line - 1 + n;
      out[at] = `${out[at] ?? ""}${text}`;
    });
    lines.set(block.file, out);
  }
  return new Map([...lines].map(([file, l]) => [file, l.join("\n")]));
}

const COLOUR = /#[0-9a-fA-F]{3,8}\b|\b(?:rgba?|hsla?|hwb|oklch|oklab|lab|lch)\(/g;
const LENGTH = /(?<![\w.#-])(\d+(?:\.\d+)?)(px|rem|em|vh|vw|vmin|vmax|ch|ex)\b/g;

/**
 * `theme-literal`: colour literals in CSS-in-TS (one finding per file and
 * value).
 *
 * @param {CssBlock[]} blocks
 * @returns {Finding[]}
 */
export function themeLiterals(blocks) {
  /** @type {Map<string, string[]>} */
  const found = new Map();
  for (const block of blocks) {
    block.css.split("\n").forEach((text, n) => {
      for (const m of text.matchAll(COLOUR)) {
        push(found, `${block.file} ${m[0]}`, `${block.file}:${String(block.line + n)}`);
      }
    });
  }
  return [...found].map(([key, places]) => ({
    check: "theme-literal",
    key,
    files: [key.split(" ")[0] ?? ""],
    detail: `${key.split(" ")[1] ?? ""}: ${places.join(", ")}`,
  }));
}

/**
 * `css-length`: a CSS length used in several CSS-in-TS files, or in one and
 * in `defaults/theme.kdl` / `defaults/layout.kdl`.
 *
 * @param {CssBlock[]} blocks
 * @param {SourceFile[]} kdl the default theme and layout
 * @returns {Finding[]}
 */
export function cssLengths(blocks, kdl) {
  /** @type {Map<string, Set<string>>} */
  const files = new Map();
  /** @type {Map<string, string[]>} */
  const places = new Map();
  /** @param {string} file @param {number} line @param {string} text */
  const scan = (file, line, text) => {
    for (const m of text.matchAll(LENGTH)) {
      const value = `${String(Number(m[1]))}${m[2] ?? ""}`;
      if (Number(m[1]) === 0) continue;
      const set = files.get(value) ?? new Set();
      set.add(file);
      files.set(value, set);
      const at = `${file}:${String(line)}`;
      if (places.get(value)?.at(-1) !== at) push(places, value, at);
    }
  };
  for (const block of blocks) {
    block.css.split("\n").forEach((text, n) => {
      scan(block.file, block.line + n, text);
    });
  }
  for (const file of kdl) {
    file.text.split("\n").forEach((text, n) => {
      scan(file.path, n + 1, text.replace(/\/\/.*$/, ""));
    });
  }
  /** @type {Finding[]} */
  const out = [];
  for (const [value, set] of files) {
    const list = [...set].sort();
    if (list.length < 2 || !list.some((f) => f.startsWith("ui/"))) continue;
    const at = places.get(value) ?? [];
    out.push({
      check: "css-length",
      key: value,
      files: list,
      detail: `${value} in ${String(list.length)} files (${String(at.length)} uses): ${at.slice(0, 8).join(", ")}${at.length > 8 ? ", …" : ""}`,
    });
  }
  return out;
}

const NUMBER = /(?<![\w.#$])(\d[\d_]*)(?![\w.])/g;

/**
 * `magic-number`: integers of at least {@link MAGIC_NUMBER_MIN} (not in
 * {@link COMMON_NUMBERS}) in the code of at least
 * {@link MAGIC_NUMBER_MIN_FILES} non-test source files of one Q2 package
 * (a number shared by Rust and TypeScript is the `rust-ts-const` check's
 * once it is named). Comments, strings and Rust test modules are skipped.
 *
 * @param {SourceFile[]} files code files
 * @returns {Finding[]}
 */
export function magicNumbers(files) {
  /** @type {Map<string, Map<string, number>>} */
  const seen = new Map();
  for (const { file, rust, text } of productionCode(files)) {
    for (const seg of lex(text, { rust })) {
      if (seg.kind !== "code") continue;
      seg.text.split("\n").forEach((codeLine, n) => {
        for (const m of codeLine.matchAll(NUMBER)) {
          const value = Number((m[1] ?? "").replace(/_/g, ""));
          if (value < MAGIC_NUMBER_MIN || COMMON_NUMBERS.has(value)) continue;
          const key = `${ownerOfPath(file.path)} ${String(value)}`;
          const at = seen.get(key) ?? /** @type {Map<string, number>} */ (new Map());
          if (!at.has(file.path)) at.set(file.path, seg.line + n);
          seen.set(key, at);
        }
      });
    }
  }
  /** @type {Finding[]} */
  const out = [];
  for (const [key, at] of seen) {
    if (at.size < MAGIC_NUMBER_MIN_FILES) continue;
    const list = [...at].sort((a, b) => a[0].localeCompare(b[0]));
    out.push({
      check: "magic-number",
      key,
      files: list.map(([f]) => f),
      detail: `${key.split(" ")[1] ?? key} in ${String(at.size)} files: ${list.map(([f, l]) => `${f}:${String(l)}`).join(", ")}`,
    });
  }
  return out;
}

/**
 * @template K, V
 * @param {Map<K, V[]>} map
 * @param {K} key
 * @param {V} value
 */
function push(map, key, value) {
  const list = map.get(key);
  if (list === undefined) map.set(key, [value]);
  else list.push(value);
}

/** @param {string[]} list */
function unique(list) {
  return [...new Set(list)].sort();
}

/** @param {ConstDef} d */
function place(d) {
  return `${d.file}:${String(d.line)}`;
}

/**
 * Drops findings with a repeated key (the first wins).
 *
 * @param {Finding[]} findings
 */
function dedupe(findings) {
  const seen = new Set();
  return findings.filter((f) => !seen.has(f.key) && seen.add(f.key));
}
