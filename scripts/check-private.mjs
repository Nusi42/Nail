#!/usr/bin/env node
// The private-data guard (ADR 0192): fails when a pattern of
// %USERPROFILE%\.config\nail\private-patterns.txt (or $NAIL_PRIVATE_PATTERNS)
// matches, case-insensitively, or the base64 form of a literal pattern
// matches. Without the file it passes ("no private patterns configured"), so
// CI and other machines work. Findings name file and line; the matched text
// is masked.
//
//   node scripts/check-private.mjs              every tracked file (content + name), `npm run check`
//   node scripts/check-private.mjs --staged     staged content + names (pre-commit hook)
//   node scripts/check-private.mjs --message F  a commit message file + the author and committer
//                                               about to be recorded (commit-msg hook)
//   node scripts/check-private.mjs --push       the commits being pushed: identities, messages,
//                                               added lines
//                                               (pre-push hook; reads git's ref lines on stdin)
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

import {
  describe,
  loadMatchers,
  maskAll,
  patternsPath,
  PUSH_LOG_FORMAT,
  pushSections,
  scanText,
} from "./lib/private-patterns.mjs";
import { messageText } from "./lib/commit-message.mjs";
import { STAGED_FILES_ARGS, parseStaged } from "./lib/staged.mjs";

// The repository the hook or check runs in (its working directory).
const root = execFileSync("git", ["rev-parse", "--show-toplevel"], { encoding: "utf8" }).trim();
const ZERO = /^0+$/;

/** @param {string[]} args */
const git = (args, input = "") =>
  execFileSync("git", args, { cwd: root, input, maxBuffer: 1 << 30 });

const file = patternsPath();
const matchers = loadMatchers(file);
if (matchers === undefined) {
  console.log("private: no private patterns configured");
  process.exit(0);
}

const [mode = "--tree", modeArg] = process.argv.slice(2);
/** @type {string[]} */
const problems = [];
/** @type {(where: string, text: string) => void} */
const check = (where, text) => {
  for (const finding of scanText(text, matchers)) {
    problems.push(describe(maskAll(where, matchers), finding));
  }
};
/** @type {(name: string) => void} */
const checkName = (name) => {
  for (const finding of scanText(name, matchers)) {
    problems.push(describe(`${maskAll(name, matchers)} (file name)`, finding));
  }
};

let scanned = "";
switch (mode) {
  case "--tree": {
    const files = git(["ls-files", "-z"]).toString("utf8").split("\0").filter(Boolean);
    for (const name of files) {
      checkName(name);
      let content;
      try {
        content = fs.readFileSync(path.join(root, name), "utf8");
      } catch {
        continue; // deleted in the working tree, or a directory (submodule)
      }
      check(name, content);
    }
    scanned = `${String(files.length)} tracked files`;
    break;
  }
  case "--staged": {
    const files = parseStaged(git(STAGED_FILES_ARGS).toString("utf8"));
    const blobs = readStaged(files);
    files.forEach((name, i) => {
      checkName(name);
      check(name, blobs[i] ?? "");
    });
    scanned = `${String(files.length)} staged files`;
    break;
  }
  case "--message": {
    if (modeArg === undefined) throw new Error("--message needs the message file");
    check("commit message", messageText(fs.readFileSync(modeArg, "utf8")));
    // The identity git records with this commit (user.name / user.email or
    // the GIT_AUTHOR_* / GIT_COMMITTER_* overrides), without the time stamp.
    for (const { who, variable } of [
      { who: "author", variable: "GIT_AUTHOR_IDENT" },
      { who: "committer", variable: "GIT_COMMITTER_IDENT" },
    ]) {
      check(`commit ${who}`, git(["var", variable]).toString("utf8").replace(/>.*$/s, ">"));
    }
    scanned = "the commit message and identity";
    break;
  }
  case "--push": {
    let commits = 0;
    for (const line of fs.readFileSync(0, "utf8").split(/\r?\n/)) {
      const [localRef, localSha, remoteRef, remoteSha] = line.trim().split(/\s+/);
      if (localRef === undefined || localSha === undefined || ZERO.test(localSha)) continue;
      checkName(`${localRef} ${remoteRef ?? ""}`);
      const range = [localSha, "--not", "--remotes"];
      if (remoteSha !== undefined && !ZERO.test(remoteSha) && known(remoteSha))
        range.push(remoteSha);
      const log = git([
        "log",
        "--no-color",
        "--no-ext-diff",
        "-p",
        PUSH_LOG_FORMAT,
        ...range,
      ]).toString("utf8");
      for (const section of pushSections(log)) check(section.where, section.text);
      commits += log.split("\0commit ").length - 1;
    }
    scanned = `${String(commits)} commits to push`;
    break;
  }
  default:
    console.error(`check-private: unknown mode ${mode}`);
    process.exit(2);
}

if (problems.length > 0) {
  for (const problem of problems) console.error(problem);
  console.error(
    `\nprivate: ${String(problems.length)} match(es) of the private patterns in ${scanned}. ` +
      "Replace them with neutral example data (example.net, the demo personas, %USERPROFILE%); " +
      "never commit with --no-verify, never copy the patterns file into the repository.",
  );
  process.exitCode = 1;
} else {
  console.log(`private: ${scanned} clean (${String(matchers.length)} matchers)`);
}

/**
 * The staged content of each file (`git cat-file --batch`).
 *
 * @param {string[]} files
 * @returns {string[]}
 */
function readStaged(files) {
  if (files.length === 0) return [];
  const out = git(["cat-file", "--batch"], files.map((f) => `:${f}\n`).join(""));
  /** @type {string[]} */
  const blobs = [];
  let at = 0;
  while (blobs.length < files.length) {
    const headerEnd = out.indexOf(10, at);
    const header = out.subarray(at, headerEnd).toString("utf8");
    const size = Number(header.split(" ")[2]);
    if (header.endsWith(" missing") || Number.isNaN(size)) {
      blobs.push("");
      at = headerEnd + 1;
      continue;
    }
    blobs.push(out.subarray(headerEnd + 1, headerEnd + 1 + size).toString("utf8"));
    at = headerEnd + 1 + size + 1;
  }
  return blobs;
}

/** @param {string} sha */
function known(sha) {
  try {
    git(["cat-file", "-e", `${sha}^{commit}`]);
    return true;
  } catch {
    return false;
  }
}
