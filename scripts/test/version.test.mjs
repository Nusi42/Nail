// Tests of scripts/lib/version.mjs. Run: node --test scripts/test/
import assert from "node:assert/strict";
import { cpSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { test } from "node:test";

import { repoRoot } from "../lib/program.mjs";
import {
  CARGO_TOML,
  PACKAGE_LOCK,
  VERSION_JSON_FILES,
  lockVersions,
  setVersion,
  versionMismatches,
  withJsonVersion,
  withWorkspaceVersion,
  workspaceRepository,
  workspaceVersion,
} from "../lib/version.mjs";

const TOML = `[workspace]
members = ["a"]

[workspace.package]
version = "1.2.3"
edition = "2024"
repository = "https://github.com/example/nail/"

[workspace.dependencies]
version = "9.9.9"
`;

test("every copy of the version is the workspace's", () => {
  const { version, mismatches } = versionMismatches(repoRoot);
  assert.match(version, /^\d+\.\d+\.\d+/);
  assert.deepEqual(mismatches, [], "set them with scripts/lib/version.mjs setVersion");
});

test("the workspace names its public repository", () => {
  const repository = workspaceRepository(readFileSync(path.join(repoRoot, CARGO_TOML), "utf8"));
  assert.match(repository, /^https:\/\/github\.com\/[^/]+\/[^/]+$/);
});

test("the workspace version and repository come from [workspace.package] only", () => {
  assert.equal(workspaceVersion(TOML), "1.2.3");
  assert.equal(workspaceRepository(TOML), "https://github.com/example/nail");
  const changed = withWorkspaceVersion(TOML, "2.0.0-beta.1");
  assert.equal(workspaceVersion(changed), "2.0.0-beta.1");
  assert.match(changed, /\[workspace\.dependencies\]\nversion = "9\.9\.9"/);
  assert.throws(() => workspaceVersion("[workspace]\n"), /no \[workspace\.package\]/);
});

test("a JSON version changes on its line only", () => {
  const json =
    '{\n  "name": "x",\n  "version": "1.0.0",\n  "deps": {\n    "version": "5"\n  }\n}\n';
  assert.equal(withJsonVersion(json, "1.1.0"), json.replace('"1.0.0"', '"1.1.0"'));
  assert.throws(() => withJsonVersion('{"name":"x"}', "1.1.0"), /no top-level version/);
});

test("setVersion writes every copy, and only those", () => {
  const dir = mkdtempSync(path.join(tmpdir(), "nail-version-"));
  try {
    for (const file of [CARGO_TOML, PACKAGE_LOCK, ...VERSION_JSON_FILES]) {
      cpSync(path.join(repoRoot, file), path.join(dir, file));
    }
    const before = readFileSync(path.join(dir, PACKAGE_LOCK), "utf8");
    const changed = setVersion(dir, "9.8.7");
    assert.deepEqual(changed.sort(), [CARGO_TOML, PACKAGE_LOCK, ...VERSION_JSON_FILES].sort());
    assert.deepEqual(versionMismatches(dir), { version: "9.8.7", mismatches: [] });
    // In the lock file only the workspaces' own version lines changed.
    const after = readFileSync(path.join(dir, PACKAGE_LOCK), "utf8");
    const diff = after.split("\n").filter((line, i) => line !== before.split("\n")[i]);
    assert.equal(diff.length, lockVersions(before).length);
    assert.deepEqual(setVersion(dir, "9.8.7"), [], "nothing changes the second time");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
