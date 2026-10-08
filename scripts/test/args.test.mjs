import assert from "node:assert/strict";
import { test } from "node:test";

import { forEachArg } from "../lib/args.mjs";

test("forEachArg hands options their values and the rest", () => {
  /** @type {string[]} */
  const seen = [];
  forEachArg(["--flag", "--out", "x", "--files", "a", "--b"], (arg, { value, rest }) => {
    if (arg === "--out") seen.push(`out=${value()}`);
    else if (arg === "--files") seen.push(`files=${rest().join(",")}`);
    else seen.push(arg);
  });
  assert.deepEqual(seen, ["--flag", "out=x", "files=a,--b"]);
});

test("forEachArg refuses a missing value or another option as a value", () => {
  /** @param {string[]} argv */
  const read = (argv) => {
    forEachArg(argv, (_arg, { value }) => {
      value();
    });
  };
  assert.throws(() => {
    read(["--out"]);
  }, /--out needs a value/);
  assert.throws(() => {
    read(["--out", "--skip"]);
  }, /--out needs a value/);
});
