import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import path from "node:path";
import { afterEach, describe, it } from "node:test";
import { fileURLToPath } from "node:url";

import { ERROR_EXIT_CODE, isMain, repoRoot, runMain } from "../lib/program.mjs";

const self = fileURLToPath(import.meta.url);

describe("program", () => {
  const saved = process.exitCode;
  afterEach(() => {
    process.exitCode = saved;
  });

  it("finds the checkout", () => {
    assert.ok(existsSync(path.join(repoRoot, "package.json")));
    assert.ok(existsSync(path.join(repoRoot, "scripts", "lib", "program.mjs")));
  });

  it("knows the program node started", () => {
    assert.equal(isMain(import.meta.url, ["node", self]), true);
    assert.equal(isMain(import.meta.url, ["node", path.join(repoRoot, "other.mjs")]), false);
    assert.equal(isMain(import.meta.url, ["node"]), false);
  });

  it("runs main only for the program, with its arguments", async () => {
    /** @type {string[][]} */
    const calls = [];
    await runMain(
      import.meta.url,
      (argv) => {
        calls.push(argv);
        return 0;
      },
      ["node", path.join(repoRoot, "other.mjs"), "--x"],
    );
    assert.deepEqual(calls, []);
    await runMain(
      import.meta.url,
      (argv) => {
        calls.push(argv);
        return 3;
      },
      ["node", self, "--x", "y"],
    );
    assert.deepEqual(calls, [["--x", "y"]]);
    assert.equal(process.exitCode, 3);
  });

  it("keeps the exit code when main answers nothing", async () => {
    process.exitCode = 0;
    await runMain(import.meta.url, () => undefined, ["node", self]);
    assert.equal(process.exitCode, 0);
  });

  it("turns an error into exit code 2", async (t) => {
    const errors = t.mock.method(console, "error", () => undefined);
    await runMain(
      import.meta.url,
      () => {
        throw new Error("broken");
      },
      ["node", self],
    );
    assert.equal(process.exitCode, ERROR_EXIT_CODE);
    assert.equal(errors.mock.calls[0]?.arguments[0], "broken");
  });
});
