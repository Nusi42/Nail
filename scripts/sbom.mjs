#!/usr/bin/env node
/**
 * Writes the CycloneDX SBOMs of a Nail build (plan M2b WP44):
 *
 *   <out>/Nail_<version>_<arch>-rust.cdx.json  Rust crates in Nail.exe and nail.exe
 *   <out>/Nail_<version>_<arch>-ui.cdx.json    npm packages in the UI bundle (package-lock.json)
 *
 * Usage: node scripts/sbom.mjs [--out DIR] [--target TRIPLE]
 * Default out: <build dir>/dist (see scripts/lib/build.mjs buildBase);
 * default target: x86_64-pc-windows-msvc. Needs cargo, no network.
 */
import { execFileSync } from "node:child_process";
import { randomUUID } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { artifactNames, assertOutsideRepo, buildBase } from "./lib/build.mjs";
import { cargoSbom, npmSbom } from "./lib/sbom.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

/** The shipped binaries' packages. */
export const ROOTS = ["nail-app", "nail-cli"];

function timestamp() {
  const epoch = process.env.SOURCE_DATE_EPOCH;
  const date = epoch ? new Date(Number(epoch) * 1000) : new Date();
  return date.toISOString().replace(/\.\d{3}Z$/, "Z");
}

export function writeSboms({
  outDir,
  target = "x86_64-pc-windows-msvc",
  arch = "x64",
  version: versionOverride,
}) {
  assertOutsideRepo(outDir, repoRoot);
  mkdirSync(outDir, { recursive: true });
  const metadata = JSON.parse(
    execFileSync(
      "cargo",
      [
        "metadata",
        "--format-version",
        "1",
        "--filter-platform",
        target,
        // CI documents exactly the committed lock file.
        ...(process.env.CI ? ["--locked"] : []),
      ],
      { cwd: repoRoot, maxBuffer: 256 * 1024 * 1024, encoding: "utf8" },
    ),
  );
  // A build of another version (build-installer.mjs --version) names its
  // SBOMs after that version.
  const version = versionOverride ?? metadata.packages.find((p) => p.name === "nail-app").version;
  const names = artifactNames(version, arch);
  const rust = cargoSbom({
    metadata,
    lockText: readFileSync(path.join(repoRoot, "Cargo.lock"), "utf8"),
    product: { name: "Nail", version },
    roots: ROOTS,
    serialNumber: `urn:uuid:${randomUUID()}`,
    timestamp: timestamp(),
  });
  const rustFile = path.join(outDir, names.sbomRust);
  writeFileSync(rustFile, JSON.stringify(rust, null, 2) + "\n");

  // The UI bundle's npm packages, from package-lock.json (`npm sbom` cannot
  // describe this workspace: the root package has no version).
  const ui = npmSbom({
    lock: JSON.parse(readFileSync(path.join(repoRoot, "package-lock.json"), "utf8")),
    workspace: "ui",
    product: { name: "Nail UI", version },
    serialNumber: `urn:uuid:${randomUUID()}`,
    timestamp: timestamp(),
  });
  const uiFile = path.join(outDir, names.sbomUi);
  writeFileSync(uiFile, JSON.stringify(ui, null, 2) + "\n");
  return { rustFile, uiFile, crates: rust.components.length, npm: ui.components.length };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const argv = process.argv.slice(2);
  const option = (name) => {
    const i = argv.indexOf(name);
    return i >= 0 ? argv[i + 1] : undefined;
  };
  const outDir = option("--out") ?? path.join(buildBase(process.env, tmpdir()), "dist");
  const result = writeSboms({ outDir, target: option("--target") });
  console.log(
    `SBOM: ${result.rustFile} (${result.crates} crates), ${result.uiFile} (${result.npm} packages)`,
  );
}
