// Tests of the duplicate / SSOT check (wave Q2, ADR 0171):
// scripts/lib/duplicates/*.mjs and scripts/check-duplicates.mjs.
// Run: node --test "scripts/test/*.test.mjs"
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";

import { failing, parseArgs } from "../lib/duplicates/cli.mjs";
import { DATA_FILES, IGNORES, JSCPD } from "../lib/duplicates/config.mjs";
import {
  CLONE_CHECKS,
  classifySsot,
  cloneFindings,
  collect,
  dataFile,
  readAllow,
} from "../lib/duplicates/index.mjs";
import { jscpdArgs, runJscpd } from "../lib/duplicates/jscpd.mjs";
import { ownerOf, ownerOfPath, PACKAGES } from "../lib/duplicates/owners.mjs";
import {
  areaCounts,
  CHECK_IDS,
  cloneClusters,
  counts,
  markdownBaseline,
  summaryText,
} from "../lib/duplicates/report.mjs";
import {
  globMatches,
  isIgnored,
  isTestPath,
  lex,
  withoutRustTests,
} from "../lib/duplicates/sources.mjs";
import {
  baselineIdentity,
  constDefs,
  cssBlocks,
  cssLengths,
  fluentDuplicates,
  fluentEntries,
  literalsOf,
  magicNumbers,
  maskedCss,
  mirrorComments,
  mirrorInventory,
  normalizeValue,
  rustTsConstants,
  rustTsLists,
  sameConstants,
  SSOT_CHECKS,
  statementAt,
  themeLiterals,
} from "../lib/duplicates/ssot.mjs";

const root = path.resolve(import.meta.dirname, "../..");

/**
 * @param {string} p
 * @param {string} text
 * @returns {import("../lib/duplicates/sources.mjs").SourceFile}
 */
const file = (p, text) => ({ path: p, text });

/**
 * @param {Partial<import("../lib/duplicates/ssot.mjs").Finding>} f
 * @returns {import("../lib/duplicates/ssot.mjs").Finding}
 */
const finding = (f) => ({ check: "const-value", key: "k", files: ["a.ts"], detail: "d", ...f });

test("ignores: generated code, vendored bindings and data, nothing else", () => {
  assert.ok(isIgnored("ui/src/api/generated/Account.ts"));
  assert.ok(isIgnored("packages/plugin-api/src/generated/index.ts"));
  assert.ok(
    isIgnored("crates/nail-store/libsqlite3-sys-sqlite3mc/bindings/bindgen_bundled_version.rs"),
  );
  assert.ok(isIgnored("crates/nail-proto-gmail/fixtures/responses/x.json"));
  assert.ok(isIgnored("crates/nail-config/tests/golden/layout_edit/a.kdl"));
  assert.ok(isIgnored("crates/nail-testkit/corpus/INBOX/1.eml"));
  assert.ok(isIgnored("e2e/visual/baselines/win32/main.png"));
  assert.ok(!isIgnored("ui/src/App.tsx"));
  assert.ok(!isIgnored("crates/nail-core/tests/m1b.rs"));
  assert.ok(globMatches("**/x/**", "x/a.ts") && globMatches("a/*.ts", "a/b.ts"));
  assert.ok(!globMatches("a/*.ts", "a/b/c.ts"));
  // Every ignore has its reason (ADR 0171 lists them).
  for (const i of IGNORES) assert.ok(i.reason.length > 10, i.glob);
});

test("test code: tests, helpers, mock servers and the mock core", () => {
  for (const f of [
    "crates/nail-core/tests/m1b.rs",
    "crates/nail-portability/src/backup/tests.rs",
    "crates/nail-store/src/thread/activity_tests.rs",
    "crates/nail-proto-ews/src/testing/ops.rs",
    "crates/nail-testkit-net/src/dav.rs",
    "ui/src/features/pim/mock.ts",
    "ui/src/api/mockMailActions.ts",
    "ui/src/features/gmail/mockSeed.ts",
    "ui/src/App.test.tsx",
    "e2e/support/nail.ts",
  ]) {
    assert.ok(isTestPath(f), f);
  }
  for (const f of [
    "crates/nail-core/src/sync.rs",
    "ui/src/features/pim/model.ts",
    "scripts/check.mjs",
  ]) {
    assert.ok(!isTestPath(f), f);
  }
});

test("lex: comments, strings, templates, raw strings and char literals", () => {
  const ts = lex('const a = "x // y"; // c\nconst b = `p ${q + `n`} s`; /* d */');
  assert.deepEqual(
    ts.filter((s) => s.kind !== "code").map((s) => [s.kind, s.text, s.line]),
    [
      ["string", "x // y", 1],
      ["comment", " c", 1],
      ["template", "p _ s", 2],
      ["comment", " d ", 2],
    ],
  );
  const rs = lex('let s = r#"a "quoted" b"#; let c = \'"\'; fn f<\'a>() {} // x', { rust: true });
  assert.deepEqual(
    rs.filter((s) => s.kind !== "code").map((s) => [s.kind, s.text]),
    [
      ["string", 'a "quoted" b'],
      ["string", '"'],
      ["comment", " x"],
    ],
  );
  assert.equal(
    withoutRustTests("fn a() {}\n#[cfg(test)]\nmod tests {\n  const X: u8 = 1;\n}\n"),
    "fn a() {}\n",
  );
});

test("mirror-comment: hand mirrors unless pinned by an existing test", () => {
  const files = [
    file(
      "ui/src/a.ts",
      "/**\n * The window model, mirrored from `nail_api::windows`.\n */\nexport {};\n",
    ),
    file("ui/src/b.ts", "// mirrors `nail_api::x` (pinned by `ui/src/b.test.ts`)\n"),
    file("ui/src/c.ts", "// The focused-composer mirror (`./sessions.ts`).\n"),
    file("crates/x/src/lib.rs", "/// Kept in sync with the UI's `fooOf` by hand.\nfn a() {}\n"),
    file("crates/y/src/lib.rs", "// mirrors the store's rows\n"),
    file("scripts/lib/duplicates/ssot.mjs", "// a Rust <-> TypeScript mirror\n"),
  ];
  const found = mirrorComments(files, (f) => f === "ui/src/b.test.ts");
  assert.deepEqual(
    found.map((f) => f.detail),
    [
      "ui/src/a.ts:2: The window model, mirrored from `nail_api::windows`.",
      "crates/x/src/lib.rs:1: Kept in sync with the UI's `fooOf` by hand.",
    ],
  );
  assert.ok(found.every((f) => f.mirror === true));
  // A pin that names a missing file does not count.
  assert.equal(mirrorComments([files[1] ?? file("", "")], () => false).length, 1);
});

test("constant definitions, values and literals", () => {
  assert.equal(normalizeValue("64 * 1_024 * 1_024; // bytes"), "64 * 1024 * 1024");
  assert.equal(normalizeValue("[11, 20] as const;"), "[11, 20]");
  assert.equal(normalizeValue("["), "");
  assert.equal(normalizeValue("foo("), "");
  const text =
    'pub const LIST: &[&str] = &[\n    "a", // x\n    "b;c",\n];\nconst N: u32 = 1_000;\n';
  assert.equal(statementAt(text, text.indexOf("= &[") + 2), '&[\n    "a", // x\n    "b;c",\n]');
  assert.deepEqual(literalsOf('&["a", "b;c"] ; 1_000', true), ['"a"', '"b;c"', "1000"]);
  const defs = constDefs(file("crates/a/src/lib.rs", text));
  assert.deepEqual(
    defs.map((d) => [d.name, d.line, d.value, d.literals]),
    [
      ["LIST", 1, "", ['"a"', '"b;c"']],
      ["N", 5, "1000", ["1000"]],
    ],
  );
  const skipped = constDefs(
    file("crates/a/src/lib.rs", "#[cfg(test)]\nmod tests {\n const T: u8 = 1;\n}\n"),
    {
      skipTests: true,
    },
  );
  assert.deepEqual(skipped, []);
});

test("rust-ts-const: one name in Rust and TypeScript, unless the values share nothing", () => {
  const files = [
    file(
      "crates/nail-api/src/a.rs",
      'pub const MAX_X: usize = 500;\npub const LISTS: &[&str] = &["inbox"];\n',
    ),
    file("ui/src/features/a/model.ts", 'export const MAX_X = 500;\nconst LISTS = ["toolbar"];\n'),
    file("ui/src/features/a/model.test.ts", "const MAX_Y = 1;\n"),
    file("crates/nail-api/src/b.rs", "pub const MAX_Y: u8 = 1;\n"),
  ];
  const found = rustTsConstants(files);
  assert.deepEqual(
    found.map((f) => [f.key, f.files]),
    [["MAX_X", ["crates/nail-api/src/a.rs", "ui/src/features/a/model.ts"]]],
  );
});

test("rust-ts-list: the same string list on both sides", () => {
  const found = rustTsLists([
    file(
      "crates/nail-api/src/a.rs",
      'const A: &[&str] = &["b", "a", "c"];\nconst B: &[&str] = &["x", "y"];\n',
    ),
    file("ui/src/a.ts", 'const A = [\n  "a",\n  "b",\n  "c",\n];\nconst B = ["x", "y"];\n'),
  ]);
  assert.deepEqual(
    found.map((f) => [f.key, f.detail]),
    [["a, b, c", "[a, b, c]: crates/nail-api/src/a.rs:1, ui/src/a.ts:1"]],
  );
});

test("const-value: the same constant in several files of one language", () => {
  const found = sameConstants([
    file("crates/a/src/x.rs", "const TIMEOUT: Duration = Duration::from_secs(20);\n"),
    file("crates/b/tests/y.rs", "const TIMEOUT: Duration = Duration::from_secs(20);\n"),
    file("crates/c/src/z.rs", "const TIMEOUT: Duration = Duration::from_secs(30);\n"),
    file("ui/src/a.ts", "const TIMEOUT = 20;\n"),
  ]);
  assert.deepEqual(
    found.map((f) => [f.key, f.files]),
    [["rs TIMEOUT = Duration::from_secs(20)", ["crates/a/src/x.rs", "crates/b/tests/y.rs"]]],
  );
});

test("fluent: entries with attributes and continuation lines; one text under several ids", () => {
  const a = file(
    "locales/en/a.ftl",
    "# c\na-cancel = Cancel\na-long =\n    One\n    two\n-term = Nail\na-dialog = Dialog\n    .title = Cancel\na-count = { $n }\n",
  );
  assert.deepEqual(
    fluentEntries(a).map((e) => [e.id, e.text, e.line]),
    [
      ["a-cancel", "Cancel", 2],
      ["a-long", "One two", 3],
      ["a-dialog", "Dialog", 7],
      ["a-dialog.title", "Cancel", 8],
    ],
  );
  const found = fluentDuplicates([
    a,
    file("locales/en/b.ftl", "b-cancel = Cancel\nb-x = One two\n"),
  ]);
  assert.deepEqual(
    found.map((f) => [f.key, f.members]),
    [
      ["Cancel", ["a-cancel", "a-dialog.title", "b-cancel"]],
      ["One two", ["a-long", "b-x"]],
    ],
  );
});

test("CSS-in-TS: blocks, masked copies with the source's lines, colours and lengths", () => {
  const skin = file(
    "ui/src/theme/skinX.ts",
    'const p = (x: string) => `[data-part="${x}"]`;\nexport const X_CSS = `\n${p("a")} { color: #fff; width: 160px; }\n${p("b")} { margin: 0; gap: 2px; }`;\n',
  );
  const other = file(
    "ui/src/features/y/skin.ts",
    "export const Y_CSS = `a { width: 160px; height: 3px; }`;\n",
  );
  const test = file("ui/src/theme/skinX.test.ts", "const css = `a { width: 160px; }`;\n");
  const blocks = cssBlocks([skin, other, test]);
  assert.deepEqual(
    blocks.map((b) => [b.file, b.line]),
    [
      ["ui/src/theme/skinX.ts", 2],
      ["ui/src/features/y/skin.ts", 1],
    ],
  );
  const masked = (maskedCss([skin, other]).get("ui/src/theme/skinX.ts") ?? "").split("\n");
  assert.equal(masked[0], "");
  assert.equal(masked[2], "var(--nail-x) { color: #fff; width: 160px; }");
  assert.deepEqual(
    themeLiterals(blocks).map((f) => f.detail),
    ["#fff: ui/src/theme/skinX.ts:3"],
  );
  const lengths = cssLengths(blocks, [
    file("defaults/theme.kdl", 'part "x" { gap "2px"; } // 3px\n'),
  ]);
  assert.deepEqual(
    lengths.map((f) => [f.key, f.files]),
    [
      ["160px", ["ui/src/features/y/skin.ts", "ui/src/theme/skinX.ts"]],
      ["2px", ["defaults/theme.kdl", "ui/src/theme/skinX.ts"]],
    ],
  );
});

test("magic-number: repeated numbers per package, code only", () => {
  const code = (/** @type {number | string} */ n) =>
    `fn f() -> u64 { ${String(n)} } // 4711\nconst S: &str = "4711";\n`;
  const found = magicNumbers([
    file("crates/nail-core/src/a.rs", code(719_468)),
    file("crates/nail-store/src/b.rs", code(719_468)),
    file("crates/nail-config/src/c.rs", code("719_468")),
    file("crates/nail-dav/src/d.rs", code(719_468)),
    file("crates/nail-core/tests/e.rs", code(719_468)),
    file("ui/src/a.ts", "const a = 1000; const b = 4711;\n"),
  ]);
  assert.deepEqual(
    found.map((f) => [f.key, f.files.length]),
    [["WP-rust-core 719468", 3]],
  );
});

test("mirror-inventory: listed mirrors are findings; missing files are problems", () => {
  const { findings, problems } = mirrorInventory(
    [
      { key: "a", files: ["x.rs", "y.ts"], what: "w" },
      { key: "b", files: ["z.ts"], what: "w", mirror: false },
    ],
    (f) => f !== "z.ts",
  );
  assert.deepEqual(
    findings.map((f) => [f.key, f.mirror]),
    [
      ["a", true],
      ["b", false],
    ],
  );
  assert.deepEqual(problems, ["mirrors.json: b: z.ts does not exist"]);
});

test("owners: every path and every finding has exactly one package", () => {
  assert.equal(ownerOfPath("crates/nail-core/src/sync.rs"), "WP-rust-core");
  assert.equal(ownerOfPath("crates/nail-util/src/time.rs"), "WP-rust-core");
  assert.equal(ownerOfPath("crates/nail-proto-imap/src/conn.rs"), "WP-rust-engines");
  assert.equal(ownerOfPath("crates/nail-net/src/lib.rs"), "WP-rust-engines");
  assert.equal(ownerOfPath("app/src-tauri/src/updater.rs"), "WP-rust-engines");
  assert.equal(ownerOfPath("ui/src/App.tsx"), "WP-ui");
  assert.equal(ownerOfPath("defaults/theme.kdl"), "WP-ui");
  assert.equal(ownerOfPath("locales/en/core-shell.ftl"), "WP-rust-core");
  assert.equal(ownerOfPath("locales/en/itip.ftl"), "WP-rust-core");
  assert.equal(ownerOfPath("locales/en/shell.ftl"), "WP-ui");
  for (const f of [
    "e2e/tests/a.spec.ts",
    "scripts/check.mjs",
    "packages/x/a.ts",
    "examples/a/b.ts",
    "eslint.config.js",
  ]) {
    assert.equal(ownerOfPath(f), "WP-mirrors-tooling", f);
  }
  assert.equal(ownerOf(["crates/nail-core/src/a.rs"], { mirror: true }), "WP-mirrors-tooling");
  assert.equal(
    ownerOf(["crates/nail-sieve/src/a.rs", "crates/nail-proto-imap/src/b.rs"]),
    "WP-rust-engines",
  );
  assert.equal(ownerOf(["ui/src/a.ts", "e2e/b.ts"]), "WP-mirrors-tooling");
  assert.equal(ownerOf(["crates/nail-core/src/a.rs", "ui/src/b.ts"]), "WP-mirrors-tooling");
  assert.equal(ownerOf(["locales/en/core.ftl", "locales/en/app.ftl"]), "WP-ui");
  assert.equal(ownerOf(["ui/a.ts", "defaults/layout.kdl"]), "WP-ui");
  for (const p of PACKAGES) assert.ok(p.startsWith("WP-"));
});

test("classify: exceptions, baseline identity, stale and pinned exceptions", () => {
  const grown = finding({ check: "const-value", key: "rs A = 1", files: ["a.rs", "b.rs", "c.rs"] });
  const known = finding({ check: "const-value", key: "rs B = 1", files: ["a.rs", "b.rs"] });
  const allowed = finding({ check: "magic-number", key: "WP-ui 480", files: ["ui/a.ts"] });
  const pinned = finding({
    check: "rust-ts-const",
    key: "MAX_X",
    files: ["x.rs", "x.ts"],
    mirror: true,
  });
  assert.equal(baselineIdentity(known), "rs B = 1 @ a.rs, b.rs");
  assert.equal(baselineIdentity(allowed), "WP-ui 480");
  const { classified, problems } = classifySsot(
    [grown, known, allowed, pinned],
    {
      "magic-number": { "WP-ui 480": { reason: "window widths" }, "WP-ui 999": { reason: "gone" } },
      "rust-ts-const": { MAX_X: { reason: "pinned", pinnedBy: "x.test.ts" } },
    },
    { version: 1, findings: { "const-value": ["rs A = 1 @ a.rs, b.rs", "rs B = 1 @ a.rs, b.rs"] } },
    (f) => (f === "x.test.ts" ? "expect(MAX_X).toBe(MAX_X_RUST)" : undefined),
    Object.keys(SSOT_CHECKS),
  );
  assert.deepEqual(
    classified.map((c) => [c.finding.key, c.status, c.owner]),
    [
      ["rs A = 1", "new", "WP-mirrors-tooling"],
      ["rs B = 1", "known", "WP-mirrors-tooling"],
      ["WP-ui 480", "allowed", "WP-ui"],
      ["MAX_X", "allowed", "WP-mirrors-tooling"],
    ],
  );
  assert.deepEqual(problems, [
    'allow.json: magic-number "WP-ui 999" matches nothing any more; remove it',
  ]);
  const missing = classifySsot(
    [pinned],
    { "rust-ts-const": { MAX_X: { reason: "pinned", pinnedBy: "gone.test.ts" } } },
    { version: 1, findings: {} },
    () => undefined,
    ["rust-ts-const"],
  );
  assert.match(missing.problems[0] ?? "", /pinning test gone\.test\.ts is missing/);
});

test("cli: arguments and which findings fail", () => {
  assert.deepEqual(parseArgs(["--strict", "--owner", "WP-ui", "--list"]), {
    strict: true,
    owner: "WP-ui",
    list: true,
    updateBaseline: false,
    markdown: undefined,
    json: undefined,
    files: [],
  });
  assert.deepEqual(parseArgs(["--files", "a\\b.rs", "--strict"]).files, ["a/b.rs", "--strict"]);
  assert.throws(() => parseArgs(["--owner", "WP-x"]), /unknown package/);
  assert.throws(() => parseArgs(["--nope"]), /unknown argument/);
  /** @type {import("../lib/duplicates/index.mjs").Classified[]} */
  const findings = [
    { finding: finding({ files: ["a.rs"] }), owner: "WP-rust-core", status: "new" },
    { finding: finding({ files: ["b.rs"] }), owner: "WP-rust-core", status: "known" },
    { finding: finding({ files: ["c.ts"] }), owner: "WP-ui", status: "allowed" },
    { finding: finding({ files: ["d.ts"] }), owner: "WP-ui", status: "new" },
  ];
  const ids = (/** @type {string[]} */ argv) =>
    failing(findings, parseArgs(argv)).map((f) => f.finding.files[0]);
  assert.deepEqual(ids([]), ["a.rs", "d.ts"]);
  assert.deepEqual(ids(["--files", "d.ts"]), ["d.ts"]);
  assert.deepEqual(ids(["--strict"]), ["a.rs", "b.rs", "d.ts"]);
  assert.deepEqual(ids(["--strict", "--owner", "WP-rust-core"]), ["a.rs", "b.rs"]);
  // A third copy of a known fragment, reported on the first two files.
  /** @type {import("../lib/duplicates/index.mjs").Classified[]} */
  const copies = [
    {
      finding: finding({ check: "clone", files: ["one.rs", "two.rs"], group: "g" }),
      owner: "WP-rust-core",
      status: "new",
    },
    {
      finding: finding({ check: "clone", files: ["one.rs", "three.rs"], group: "g" }),
      owner: "WP-rust-core",
      status: "known",
    },
    {
      finding: finding({ check: "clone", files: ["x.rs", "y.rs"], group: "h" }),
      owner: "WP-rust-core",
      status: "new",
    },
  ];
  assert.deepEqual(
    failing(copies, parseArgs(["--files", "three.rs"])).map((f) => f.finding.files.join(" ")),
    ["one.rs two.rs"],
  );
});

test("reports: counts per check and package, clusters, areas, Markdown", () => {
  /** @type {import("../lib/duplicates/index.mjs").Classified[]} */
  const findings = [
    {
      finding: finding({
        check: "clone",
        files: ["crates/a/src/x.rs", "crates/a/src/y.rs"],
        detail: "x <> y (12 lines, 90 tokens)",
      }),
      owner: "WP-rust-core",
      status: "known",
    },
    {
      finding: finding({
        check: "clone",
        files: ["crates/a/src/x.rs", "crates/a/src/y.rs"],
        detail: "x <> y (8 lines, 60 tokens)",
      }),
      owner: "WP-rust-core",
      status: "new",
    },
    {
      finding: finding({ check: "fluent-text", files: ["locales/en/a.ftl"] }),
      owner: "WP-ui",
      status: "known",
    },
  ];
  assert.deepEqual(counts(findings).get("clone")?.get("WP-rust-core"), { known: 1, new: 1 });
  assert.deepEqual(CHECK_IDS, [...Object.keys(CLONE_CHECKS), ...Object.keys(SSOT_CHECKS)]);
  assert.match(summaryText(findings), /clone\s+2\s+new\s+1\s+\(rust-core 2\)/);
  assert.deepEqual(cloneClusters(findings, 5), [
    { pair: "crates/a/src/x.rs <> crates/a/src/y.rs", clones: 2, lines: 20, owner: "WP-rust-core" },
  ]);
  assert.deepEqual(areaCounts(findings), [
    ["crates/a", 2],
    ["locales/en", 1],
  ]);
  const md = markdownBaseline(findings, { files: 3, lines: 100, duplicatedLines: 20 });
  assert.match(md, /\| clone \| 2 \| 0 \| 0 \| 0 \| 2 \|/);
  assert.match(
    md,
    /\| crates\/a\/src\/x\.rs <> crates\/a\/src\/y\.rs \| 2 \| 20 \| WP-rust-core \|/,
  );
});

test("jscpd: arguments from the one configuration", () => {
  const args = jscpdArgs({ paths: ["."], out: "o", baseline: "b.json" });
  assert.equal(args[args.indexOf("--min-tokens") + 1], String(JSCPD.minTokens));
  assert.equal(args[args.indexOf("--min-lines") + 1], String(JSCPD.minLines));
  assert.equal(args[args.indexOf("--format") + 1], JSCPD.formats.join(","));
  assert.equal(args[args.indexOf("--ignore") + 1], IGNORES.map((i) => i.glob).join(","));
  assert.equal(args[args.indexOf("--baseline") + 1], "b.json");
  assert.ok(!args.includes("--update-baseline"));
  const css = jscpdArgs({
    paths: ["."],
    out: "o",
    ignore: false,
    formats: ["css"],
    baseline: "c",
    updateBaseline: true,
  });
  assert.ok(
    css.includes("--no-gitignore") &&
      !css.includes("--ignore") &&
      css.includes("--update-baseline"),
  );
});

test("jscpd finds a copied function and its baseline ratchets it", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "nail-dup-"));
  try {
    const body = Array.from(
      { length: 8 },
      (_, i) =>
        `fn step${String(i)}(x: u32) -> u32 { let y = x + ${String(i)}; if y > 10 { return y * 2; } y - 1 }`,
    ).join("\n");
    fs.mkdirSync(path.join(dir, "src"));
    fs.writeFileSync(path.join(dir, "src", "a.rs"), `${body}\n`);
    fs.writeFileSync(path.join(dir, "src", "b.rs"), `// copied\n${body}\n`);
    const baseline = path.join(dir, "baseline.json");
    const scan = (/** @type {boolean} */ update) =>
      runJscpd(root, dir, {
        paths: ["."],
        out: path.join(dir, "out"),
        ignore: false,
        // jscpd refuses a missing baseline unless it is to write one.
        baseline: update || fs.existsSync(baseline) ? baseline : undefined,
        updateBaseline: update,
      });
    const fresh = cloneFindings(scan(false), "clone", { baseline: fs.existsSync(baseline) });
    assert.equal(fresh.length, 1);
    assert.ok(fresh[0]?.isNew);
    assert.deepEqual(fresh[0].files, ["src/a.rs", "src/b.rs"]);
    scan(true);
    const again = cloneFindings(scan(false), "clone", { baseline: true });
    assert.equal(again[0]?.isNew, false);
    // Without a baseline file every clone is new.
    assert.ok(cloneFindings(scan(false), "clone", { baseline: false })[0]?.isNew);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("the repository's data files are valid", () => {
  const allow = readAllow(dataFile(root, DATA_FILES.allow));
  for (const [check, entries] of Object.entries(allow)) {
    assert.ok(check in SSOT_CHECKS, `allow.json: unknown check ${check}`);
    for (const [key, entry] of Object.entries(entries)) assert.ok(entry.reason.length > 10, key);
  }
});

test("the whole check runs over the checkout and fails on nothing new", () => {
  const result = collect(root, { buildDir: path.join(root, ".nail-build") });
  assert.ok(result.stats.files > 1000);
  assert.deepEqual(result.problems, []);
  const fresh = result.findings.filter((f) => f.status === "new");
  assert.deepEqual(
    fresh.map((f) => `${f.finding.check}: ${f.finding.detail}`),
    [],
    "new duplicates: run `node scripts/check-duplicates.mjs`",
  );
  const cli = spawnSync(process.execPath, [path.join(root, "scripts", "check-duplicates.mjs")], {
    cwd: root,
    encoding: "utf8",
  });
  assert.equal(cli.status, 0, cli.stderr);
  assert.match(cli.stdout, /duplicates: findings per check/);
});
