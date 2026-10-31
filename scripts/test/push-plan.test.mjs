// Tests of scripts/lib/push-plan.mjs (ADR 0205): every combination of
// checkout role, remote URL and pushed refs. The hook itself, run by git, is
// tested in scripts/test/hooks.test.mjs. Run: node --test scripts/test/
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { pathToFileURL } from "node:url";

import {
  PUBLIC_REMOTE,
  checkoutRole,
  isLocalUrl,
  isReleaseTagRef,
  localPath,
  masterOf,
  parsePushLines,
  publishCommand,
  publishPushes,
  pushPlan,
} from "../lib/push-plan.mjs";
import { NETWORK_URL, ZERO_SHA, git, initRepo, withTempDir } from "./temp-repo.mjs";

const SHA = "1".repeat(40);
const LOCAL = "C:\\w\\master";

/**
 * @param {string} localRef
 * @param {string} [remoteRef]
 */
const ref = (localRef, remoteRef = localRef) => ({
  localRef,
  localSha: SHA,
  remoteRef,
  remoteSha: ZERO_SHA,
});
const MAIN = ref("refs/heads/main");

/** @param {ReturnType<typeof pushPlan>} plan */
const stepIds = (plan) => ("steps" in plan ? plan.steps.map((s) => s.id) : plan.refusal);

test("paths are local remotes, everything else is a network remote", () => {
  for (const url of [
    "C:\\Users\\x\\nail",
    "c:/x/nail",
    "/home/x/nail",
    "../nail",
    "./nail",
    "..",
    "\\\\host\\share\\nail",
    "file:///C:/x/nail",
    "FILE://host/share",
  ]) {
    assert.equal(isLocalUrl(url), true, url);
  }
  for (const url of [
    NETWORK_URL,
    "http://example.org/x",
    "ssh://git@example.org/x.git",
    "git@example.org:x/nail.git",
    "git://example.org/x",
    "example.org:x",
  ]) {
    assert.equal(isLocalUrl(url), false, url);
  }
});

test("a local URL names a directory relative to the checkout", () => {
  const base = path.resolve("/w/nail-work-1");
  assert.equal(localPath(base, "../master"), path.resolve("/w/master"));
  const master = path.resolve("/w/master");
  assert.equal(localPath(base, pathToFileURL(master).href), master);
});

test("git's ref lines on stdin are parsed, incomplete lines skipped", () => {
  assert.deepEqual(
    parsePushLines(`refs/heads/main ${SHA} refs/heads/main ${ZERO_SHA}\r\n\nbroken line\n`),
    [MAIN],
  );
});

test("release tags are v<semantic version> under refs/tags", () => {
  assert.equal(isReleaseTagRef("refs/tags/v0.8.0"), true);
  assert.equal(isReleaseTagRef("refs/tags/v1.2.3-beta.1"), true);
  assert.equal(isReleaseTagRef("refs/tags/0.8.0"), false);
  assert.equal(isReleaseTagRef("refs/tags/vnext"), false);
  assert.equal(isReleaseTagRef("refs/heads/v0.8.0"), false);
});

test("a work folder pushes to the master with the full check", () => {
  assert.deepEqual(stepIds(pushPlan({ role: "work", url: LOCAL, refs: [MAIN] })), [
    "private-push",
    "check",
  ]);
  assert.deepEqual(
    stepIds(pushPlan({ role: "work", url: "../master", refs: [ref("refs/heads/work-3")] })),
    ["private-push", "check"],
  );
});

test("a work folder never pushes to a network remote; the refusal names the master command", () => {
  for (const url of [NETWORK_URL, "git@example.org:x/nail.git", "ssh://example.org/x"]) {
    const plan = pushPlan({ role: "work", url, refs: [MAIN], master: LOCAL });
    assert.ok("refusal" in plan, url);
    assert.ok(plan.refusal.includes(publishCommand(LOCAL)), plan.refusal);
  }
  assert.equal(publishCommand(LOCAL), `git -C "${LOCAL}" push ${PUBLIC_REMOTE} main`);
});

test("the master publishes main and release tags with checks that build nothing", () => {
  const steps = stepIds(
    pushPlan({ role: "master", url: NETWORK_URL, refs: [MAIN, ref("refs/tags/v0.8.0")] }),
  );
  assert.deepEqual(steps, ["private-push", "private", "licenses"]);
  const plan = pushPlan({ role: "master", url: NETWORK_URL, refs: [MAIN] });
  assert.ok("steps" in plan);
  // The guard reads git's ref lines; the licence check is cargo deny's alone.
  assert.equal(plan.steps[0]?.stdin, true);
  assert.deepEqual(plan.steps[2]?.args, ["--locked", "check", "licenses"]);
  assert.ok(plan.steps.every((s) => !s.args.includes("scripts/check.mjs")));
});

test("the master pushes nothing but main and release tags", () => {
  for (const refs of [
    [ref("refs/heads/work-1")],
    [ref("refs/heads/main", "refs/heads/other")],
    [ref("refs/heads/work-1", "refs/heads/main")],
    [MAIN, ref("refs/tags/next")],
    [ref("refs/tags/v0.8.0", "refs/tags/v0.9.0")],
    [{ ...MAIN, localRef: "(delete)", localSha: ZERO_SHA }],
  ]) {
    const plan = pushPlan({ role: "master", url: NETWORK_URL, refs });
    assert.ok("refusal" in plan, JSON.stringify(refs));
    assert.match(plan.refusal, /publishes only main/);
  }
});

test("the master never pushes to a local path", () => {
  for (const url of [LOCAL, "../nail-work-1", "file:///C:/w/x"]) {
    const plan = pushPlan({ role: "master", url, refs: [MAIN] });
    assert.ok("refusal" in plan, url);
    assert.match(plan.refusal, /only to the public remote/);
  }
});

test("a plain clone (no master) keeps the full check for every remote", () => {
  for (const url of [NETWORK_URL, LOCAL]) {
    assert.deepEqual(stepIds(pushPlan({ role: "plain", url, refs: [MAIN] })), [
      "private-push",
      "check",
    ]);
  }
});

test("a release is published from the master when there is one, else from here", () => {
  const o = { branch: "work-1", tag: "v0.9.0", remote: "github", origin: LOCAL };
  assert.deepEqual(publishPushes({ ...o, master: LOCAL }), [
    ["push", "origin", "work-1", "v0.9.0"],
    ["-C", LOCAL, "merge", "--ff-only", "work-1"],
    ["-C", LOCAL, "push", "github", "main", "v0.9.0"],
  ]);
  assert.deepEqual(publishPushes(o), [
    ["push", "origin", "work-1", "v0.9.0"],
    ["push", "github", "work-1:main", "v0.9.0"],
  ]);
  assert.deepEqual(publishPushes({ ...o, origin: undefined }), [
    ["push", "github", "work-1:main", "v0.9.0"],
  ]);
});

test("the role comes from the checkout's .env.local and its origin", () => {
  withTempDir("nail-push-plan-", (base) => {
    const master = initRepo(path.join(base, "master"));
    const work = path.join(base, "work");
    git(base, ["clone", "-q", master, work]);
    const plain = initRepo(path.join(base, "plain"));
    git(plain, ["remote", "add", "origin", NETWORK_URL]);
    /** @type {NodeJS.ProcessEnv} */
    const env = {};

    // Before the master is marked, its clones are plain clones.
    assert.equal(checkoutRole(work, env), "plain");
    fs.writeFileSync(path.join(master, ".env.local"), "NAIL_CHECKOUT_ROLE=master\n");
    assert.equal(checkoutRole(master, env), "master");
    assert.equal(checkoutRole(work, env), "work");
    assert.equal(masterOf(work), path.resolve(master));
    assert.equal(checkoutRole(plain, env), "plain");
    assert.equal(masterOf(plain), undefined);
  });
});
