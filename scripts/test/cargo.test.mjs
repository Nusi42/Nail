import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { after, describe, it } from "node:test";

import {
  availableGib,
  cargoJobs,
  ensureCargoJobs,
  GIB,
  GIB_PER_JOB,
  MIN_JOBS,
  RESERVED_GIB,
  workspaceBinary,
} from "../lib/cargo.mjs";

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

describe("cargoJobs", () => {
  it("takes one job per share of the free memory after the reserve", () => {
    const availableGib = RESERVED_GIB + 5 * GIB_PER_JOB + 1;
    assert.equal(cargoJobs({ availableGib, cpus: 32 }), 5);
  });

  it("never takes more jobs than hardware threads", () => {
    assert.equal(cargoJobs({ availableGib: 1000, cpus: 6 }), 6);
  });

  it("keeps the minimum when memory is short", () => {
    assert.equal(cargoJobs({ availableGib: 1, cpus: 32 }), MIN_JOBS);
    assert.equal(cargoJobs({ availableGib: 1000, cpus: 1 }), MIN_JOBS);
  });
});

describe("availableGib", () => {
  it("takes the smaller of free memory and free commit on Windows", () => {
    const probe = {
      platform: /** @type {const} */ ("win32"),
      freemem: 20 * GIB,
      totalmem: 64 * GIB,
    };
    assert.equal(availableGib({ ...probe, freeCommit: () => 8 * GIB }), 8);
    assert.equal(availableGib({ ...probe, freeCommit: () => 30 * GIB }), 20);
    assert.equal(availableGib({ ...probe, freeCommit: () => null }), 20);
  });

  it("counts half the memory as free on macOS when less shows as free", () => {
    const probe = { platform: /** @type {const} */ ("darwin"), totalmem: 32 * GIB };
    assert.equal(availableGib({ ...probe, freemem: 2 * GIB }), 16);
    assert.equal(availableGib({ ...probe, freemem: 20 * GIB }), 20);
  });

  it("takes the free memory on Linux", () => {
    assert.equal(availableGib({ platform: "linux", freemem: 12 * GIB, totalmem: 64 * GIB }), 12);
  });
});

describe("ensureCargoJobs", () => {
  it("keeps an explicit CARGO_BUILD_JOBS", () => {
    const env = { CARGO_BUILD_JOBS: "3" };
    assert.equal(
      ensureCargoJobs(env, () => assert.fail("memory is not asked")),
      "3",
    );
  });

  it("sets CARGO_BUILD_JOBS from the free memory", () => {
    /** @type {Partial<Record<string, string>>} */
    const env = {};
    const jobs = ensureCargoJobs(env, () => 0);
    assert.equal(jobs, String(MIN_JOBS));
    assert.equal(env.CARGO_BUILD_JOBS, jobs);
  });
});
