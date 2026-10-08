import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { after, describe, it } from "node:test";

import { workspaceBinary } from "../lib/cargo.mjs";

describe("workspaceBinary", () => {
  const targetDir = mkdtempSync(path.join(tmpdir(), "nail-cargo-test-"));
  after(() => {
    rmSync(targetDir, { recursive: true, force: true });
  });
  const exe = process.platform === "win32" ? "nail.exe" : "nail";
  /** @type {import("../lib/cargo.mjs").BinarySpec} */
  const spec = {
    repoRoot: "unused",
    name: "nail",
    pkg: "nail-cli",
    profile: "release",
    envName: "NAIL_BIN",
    targetDir,
  };

  it("prefers the environment variable", () => {
    assert.equal(workspaceBinary({ ...spec, env: { NAIL_BIN: "C:/x/nail.exe" } }), "C:/x/nail.exe");
  });

  it("names the build command when the binary is missing", () => {
    assert.throws(
      () => workspaceBinary({ ...spec, env: {} }),
      /run `cargo build --release -p nail-cli` first/,
    );
    assert.throws(
      () => workspaceBinary({ ...spec, profile: "debug", env: {} }),
      /run `cargo build -p nail-cli` first/,
    );
  });

  it("finds the built binary in the profile's directory", () => {
    mkdirSync(path.join(targetDir, "debug"), { recursive: true });
    writeFileSync(path.join(targetDir, "debug", exe), "");
    assert.equal(
      workspaceBinary({ ...spec, profile: "debug", env: {} }),
      path.join(targetDir, "debug", exe),
    );
  });
});
