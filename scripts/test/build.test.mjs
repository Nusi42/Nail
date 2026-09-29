// Tests of scripts/lib/build.mjs. Run: node --test scripts/test/
import assert from "node:assert/strict";
import path from "node:path";
import { test } from "node:test";

import {
  artifactNames,
  assertOutsideRepo,
  buildBase,
  checksumLines,
  installerConfigOverlay,
  parseArgs,
  portableLayout,
  tauriArch,
} from "../lib/build.mjs";

test("build output goes to NAIL_BUILD_DIR, LOCALAPPDATA or tmp", () => {
  assert.equal(buildBase({ NAIL_BUILD_DIR: "X", LOCALAPPDATA: "L" }, "T"), "X");
  assert.equal(buildBase({ LOCALAPPDATA: "L" }, "T"), path.join("L", "nail-build"));
  assert.equal(buildBase({}, "T"), path.join("T", "nail-build"));
});

test("output inside the repository is refused, except .nail-build", () => {
  const repo = path.resolve("repo");
  assert.throws(() => assertOutsideRepo(repo, repo), /outside the repository/);
  assert.throws(() => assertOutsideRepo(path.join(repo, "target"), repo), /outside/);
  assert.doesNotThrow(() => assertOutsideRepo(path.join(repo, ".nail-build", "dist"), repo));
  assert.doesNotThrow(() => assertOutsideRepo(path.resolve("elsewhere"), repo));
  assert.doesNotThrow(() => assertOutsideRepo(path.resolve("repo-2"), repo));
});

test("options", () => {
  assert.deepEqual(parseArgs([]), { skipUi: false, targetDir: null, outDir: null, cli: true });
  assert.deepEqual(parseArgs(["--skip-ui", "--no-cli", "--target-dir", "t", "--out", "o"]), {
    skipUi: true,
    targetDir: "t",
    outDir: "o",
    cli: false,
  });
  assert.throws(() => parseArgs(["--out"]), /needs a value/);
  assert.throws(() => parseArgs(["--out", "--skip-ui"]), /needs a value/);
  assert.throws(() => parseArgs(["--sign"]), /unknown option/);
});

test("the overlay puts the CLI into bin and disables the UI build", () => {
  assert.deepEqual(installerConfigOverlay({ cliPath: "C:\\t\\nail.exe" }), {
    build: { beforeBuildCommand: null },
    bundle: { resources: { "C:\\t\\nail.exe": "bin/nail.exe" } },
  });
  assert.deepEqual(installerConfigOverlay({ cliPath: null }), {
    build: { beforeBuildCommand: null },
  });
});

test("artifact names follow the Tauri bundler", () => {
  assert.equal(tauriArch("x64"), "x64");
  assert.equal(tauriArch("arm64"), "arm64");
  assert.throws(() => tauriArch("mips"), /unsupported/);
  assert.deepEqual(artifactNames("0.1.0", "x64"), {
    installer: "Nail_0.1.0_x64-setup.exe",
    portableZip: "Nail_0.1.0_x64-portable.zip",
    sbomRust: "Nail_0.1.0_x64-rust.cdx.json",
    sbomUi: "Nail_0.1.0_x64-ui.cdx.json",
    checksums: "Nail_0.1.0_x64-SHA256SUMS.txt",
  });
});

test("the portable zip has the marker and the CLI in bin", () => {
  assert.deepEqual(portableLayout({ appExe: "a.exe", cliExe: "n.exe" }), [
    { from: "a.exe", to: "Nail.exe" },
    { from: "n.exe", to: "bin/nail.exe" },
    { content: "", to: "portable" },
  ]);
  assert.equal(portableLayout({ appExe: "a.exe", cliExe: null }).length, 2);
});

test("checksum lines are sorted and sha256sum-compatible", () => {
  assert.equal(
    checksumLines([
      { name: "b", sha256: "22" },
      { name: "a", sha256: "11" },
    ]),
    "11  a\n22  b\n",
  );
});
