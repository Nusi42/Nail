// The private-data guard (ADR 0192). Synthetic patterns only: never put the
// owner's real patterns (or anything derived from them) into a test.
// Run: node --test scripts/test/
import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  MIN_ENCODED_LENGTH,
  PATTERNS_ENV,
  base64Forms,
  describe,
  literalOf,
  loadMatchers,
  mask,
  maskAll,
  matchersFrom,
  patternsPath,
  pushSections,
  scanText,
} from "../lib/private-patterns.mjs";

const SCRIPT = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
  "check-private.mjs",
);

const PATTERNS = [
  "# synthetic patterns",
  "jo@corp\\.example\\.net",
  "\\bcorp\\.example\\.net\\b",
  "",
  "\\bjoanne\\b",
  "inv[0-9]+x",
].join("\n");

test("literal patterns are recognised; classes and operators are not", () => {
  assert.equal(literalOf("jo@corp\\.example\\.net"), "jo@corp.example.net");
  assert.equal(literalOf("\\bjoanne\\b"), "joanne");
  assert.equal(literalOf("^joanne$"), "joanne");
  assert.equal(literalOf("inv[0-9]+x"), undefined);
  assert.equal(literalOf("a\\dz"), undefined);
  assert.equal(literalOf("a|b"), undefined);
  assert.equal(literalOf("\\b"), undefined);
});

test("base64 forms are found at every byte alignment", () => {
  const literal = "joanne";
  const forms = base64Forms(literal);
  assert.ok(forms.every((f) => f.length >= MIN_ENCODED_LENGTH));
  for (const prefix of ["", "a", "ab", "abc", "user=", "AUTH\0"]) {
    for (const suffix of ["", "!", "xy", "\0\0\0"]) {
      const encoded = Buffer.from(`${prefix}${literal}${suffix}`).toString("base64");
      assert.ok(
        forms.some((f) => encoded.includes(f)),
        `no form found in base64 of ${JSON.stringify(prefix + literal + suffix)}`,
      );
    }
  }
  // Too short to encode safely: no forms.
  assert.deepEqual(base64Forms("ab"), []);
});

test("URL-safe base64 forms are searched too", () => {
  // "???>>>" encodes with '/' and '+' at offset 0.
  const forms = base64Forms("???>>>");
  assert.ok(forms.some((f) => f.includes("/") || f.includes("+")));
  assert.ok(forms.some((f) => f.includes("_") || f.includes("-")));
});

test("matchers: patterns case-insensitive, base64 forms exact, comments skipped", () => {
  const matchers = matchersFrom(PATTERNS);
  assert.equal(matchers.filter((m) => m.kind === "pattern").length, 4);
  assert.ok(matchers.some((m) => m.kind === "base64" && m.line === 5));
  assert.ok(!matchers.some((m) => m.kind === "base64" && m.line === 6));
  const text = [
    "fine line",
    "mail JO@Corp.Example.NET today",
    `token ${Buffer.from("user=joanne;").toString("base64")}`,
    "see INV42X and joannes (no word boundary)",
  ].join("\n");
  const findings = scanText(text, matchers);
  const lines = findings.map((f) => f.line);
  assert.ok(lines.includes(2));
  assert.ok(lines.includes(3));
  assert.ok(lines.includes(4));
  assert.ok(!lines.includes(1));
  const mail = findings.find((f) => f.line === 2 && f.matcher.line === 2);
  assert.ok(mail);
  assert.equal(mail.column, 6);
  assert.equal(mail.masked, `J${"*".repeat("jo@corp.example.net".length - 1)}`);
  // "joannes" does not match \bjoanne\b.
  assert.ok(
    !findings.some((f) => f.line === 4 && f.matcher.line === 5 && f.matcher.kind === "pattern"),
  );
});

test("an invalid pattern is reported by line, never echoed", () => {
  assert.throws(
    () => matchersFrom("ok\n(secret-unclosed"),
    (error) => {
      assert.ok(error instanceof Error);
      assert.match(error.message, /line 2/);
      assert.doesNotMatch(error.message, /secret/);
      return true;
    },
  );
});

test("output masks every match", () => {
  const matchers = matchersFrom(PATTERNS);
  assert.equal(mask("joanne"), "j*****");
  assert.equal(maskAll("docs/joanne/notes.md", matchers), "docs/j*****/notes.md");
  const [finding] = scanText("x joanne", matchers);
  assert.ok(finding);
  const line = describe("a.md", finding);
  assert.equal(line, 'a.md:1:3: private pattern 5 matched "j*****"');
  assert.doesNotMatch(line, /joanne/);
});

test("the patterns file: environment override, absent file = nothing to check", () => {
  assert.equal(patternsPath({ [PATTERNS_ENV]: "x.txt" }), "x.txt");
  assert.match(patternsPath({}), /\.nail-private[\\/]patterns\.txt$/);
  assert.equal(loadMatchers(path.join(os.tmpdir(), "nail-no-such-patterns.txt")), undefined);
});

test("push sections: identities, messages, file names and added lines only", () => {
  const log = [
    "\0commit 1111111111111111111111111111111111111111",
    "Sam <sam@example.net>",
    "Sam <sam@example.net>",
    "Fix: remove joanne's address",
    "",
    "diff --git a/docs/a.md b/docs/a.md",
    "--- a/docs/a.md",
    "+++ b/docs/a.md",
    "@@ -1 +1 @@",
    "-old jo@corp.example.net",
    "+new sam@example.net",
    "\0commit 2222222222222222222222222222222222222222",
    "Joanne <jo@corp.example.net>",
    "Sam <sam@example.net>",
    "Second",
    "",
    "diff --git a/notes/joanne.txt b/notes/joanne.txt",
    "new file mode 100644",
    "--- /dev/null",
    "+++ b/notes/joanne.txt",
    "@@ -0,0 +1 @@",
    "+hello",
  ].join("\n");
  const sections = pushSections(log);
  const matchers = matchersFrom(PATTERNS);
  const hits = sections.flatMap((s) => scanText(s.text, matchers).map(() => s.where));
  assert.deepEqual(hits, [
    "commit 111111111111 message",
    "commit 222222222222 author",
    "commit 222222222222 author",
    "commit 222222222222 author",
    "commit 222222222222 notes/joanne.txt (file name)",
  ]);
});

/**
 * @param {string} dir
 * @param {string[]} args
 */
const git = (dir, args) => execFileSync("git", args, { cwd: dir, encoding: "utf8" });

/**
 * @param {string} dir
 * @param {string[]} args
 * @param {string | undefined} patterns
 * @param {string} [input]
 */
const run = (dir, args, patterns, input = "") =>
  spawnSync(process.execPath, [SCRIPT, ...args], {
    cwd: dir,
    input,
    encoding: "utf8",
    env: { ...process.env, [PATTERNS_ENV]: patterns ?? path.join(dir, "absent.txt") },
  });

test("the command checks the tree, staged files, messages and pushes", () => {
  const base = fs.mkdtempSync(path.join(os.tmpdir(), "nail-private-"));
  try {
    const patterns = path.join(base, "patterns.txt");
    fs.writeFileSync(patterns, PATTERNS);
    const repo = path.join(base, "repo");
    fs.mkdirSync(repo);
    git(repo, ["init", "-q", "-b", "main"]);
    git(repo, ["config", "user.email", "sam@example.net"]);
    git(repo, ["config", "user.name", "Sam"]);
    git(repo, ["config", "core.hooksPath", "no-hooks"]);
    fs.writeFileSync(path.join(repo, "a.txt"), "clean\n");
    git(repo, ["add", "a.txt"]);
    git(repo, ["commit", "-q", "-m", "clean"]);

    // No patterns file: passes and says so.
    const absent = run(repo, [], undefined);
    assert.equal(absent.status, 0);
    assert.match(absent.stdout, /no private patterns configured/);

    assert.equal(run(repo, [], patterns).status, 0);

    // Staged content (base64-encoded) and a staged file name.
    fs.writeFileSync(
      path.join(repo, "b.txt"),
      `x\ny ${Buffer.from("name=joanne").toString("base64")}\n`,
    );
    fs.writeFileSync(path.join(repo, "joanne.txt"), "x\n");
    git(repo, ["add", "b.txt", "joanne.txt"]);
    const staged = run(repo, ["--staged"], patterns);
    assert.equal(staged.status, 1);
    assert.match(staged.stderr, /^b\.txt:2:\d+: the base64 form of private pattern 5/m);
    assert.match(staged.stderr, /j\*{5}\.txt \(file name\)/);
    assert.doesNotMatch(staged.stderr, /joanne/);

    // The working tree copy is not what is committed: staged content counts.
    fs.writeFileSync(path.join(repo, "b.txt"), "fixed\n");
    assert.equal(run(repo, ["--staged"], patterns).status, 1);
    git(repo, ["rm", "-q", "--cached", "joanne.txt"]);
    git(repo, ["add", "b.txt"]);
    assert.equal(run(repo, ["--staged"], patterns).status, 0);

    // Commit messages (comment lines are not part of the message).
    const message = path.join(base, "MSG");
    fs.writeFileSync(message, "Subject\n\n# joanne in a comment\n");
    assert.equal(run(repo, ["--message", message], patterns).status, 0);
    fs.writeFileSync(message, "Mail to jo@corp.example.net\n");
    const bad = run(repo, ["--message", message], patterns);
    assert.equal(bad.status, 1);
    assert.match(bad.stderr, /^commit message:1:9: private pattern 2/m);
    // The author and committer about to be recorded count too.
    fs.writeFileSync(message, "Subject\n");
    const author = spawnSync(process.execPath, [SCRIPT, "--message", message], {
      cwd: repo,
      encoding: "utf8",
      env: { ...process.env, [PATTERNS_ENV]: patterns, GIT_AUTHOR_NAME: "Joanne" },
    });
    assert.equal(author.status, 1);
    assert.match(author.stderr, /^commit author:1:1: private pattern 5/m);
    assert.doesNotMatch(author.stderr, /commit committer/);

    // The tree check sees committed files.
    git(repo, ["commit", "-q", "-m", "b"]);
    fs.writeFileSync(path.join(repo, "c.txt"), "corp.example.net\n");
    git(repo, ["add", "c.txt"]);
    git(repo, ["commit", "-q", "-m", "c"]);
    const tree = run(repo, [], patterns);
    assert.equal(tree.status, 1);
    assert.match(tree.stderr, /^c\.txt:1:1: private pattern 3/m);

    // Push: the commits not on any remote, as git's pre-push hook lists them.
    const head = git(repo, ["rev-parse", "HEAD"]).trim();
    const zero = "0".repeat(40);
    const push = run(
      repo,
      ["--push"],
      patterns,
      `refs/heads/main ${head} refs/heads/main ${zero}\n`,
    );
    assert.equal(push.status, 1);
    assert.match(push.stderr, /c\.txt \(added lines/);
    assert.doesNotMatch(push.stderr, /author|committer/);
    // A commit recorded with a private identity.
    git(repo, ["-c", "user.name=joanne", "commit", "-q", "--allow-empty", "-m", "d"]);
    const named = git(repo, ["rev-parse", "HEAD"]).trim();
    const pushed = run(
      repo,
      ["--push"],
      patterns,
      `refs/heads/main ${named} refs/heads/main ${zero}\n`,
    );
    assert.match(pushed.stderr, /^commit [0-9a-f]{12} author:1:1: private pattern 5/m);
    assert.match(pushed.stderr, /^commit [0-9a-f]{12} committer:1:1: private pattern 5/m);
    // A deleted ref pushes nothing.
    const deleted = run(repo, ["--push"], patterns, `(delete) ${zero} refs/heads/x ${head}\n`);
    assert.equal(deleted.status, 0);
  } finally {
    fs.rmSync(base, { recursive: true, force: true });
  }
});
