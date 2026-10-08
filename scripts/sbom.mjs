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
import path from "node:path";
import { forEachArg } from "./lib/args.mjs";
import { artifactNames, assertOutsideRepo, buildBase } from "./lib/build.mjs";
import { parseJson, readJson } from "./lib/json.mjs";
import { repoRoot, runMain } from "./lib/program.mjs";
import { cargoSbom, npmSbom } from "./lib/sbom.mjs";

/** The shipped binaries' packages. */
export const ROOTS = ["nail-app", "nail-cli"];

function timestamp() {
  const epoch = process.env.SOURCE_DATE_EPOCH;
  const date = epoch ? new Date(Number(epoch) * 1000) : new Date();
  return date.toISOString().replace(/\.\d{3}Z$/, "Z");
}

/**
 * Writes both SBOMs into `outDir` (outside the repository).
 *
 * @param {{ outDir: string, target?: string, arch?: string, version?: string }} options
 */
export function writeSboms({
  outDir,
  target = "x86_64-pc-windows-msvc",
  arch = "x64",
  version: versionOverride,
}) {
  assertOutsideRepo(outDir, repoRoot);
  mkdirSync(outDir, { recursive: true });
  const metadata = /** @type {import("./lib/sbom.mjs").CargoMetadata} */ (
    parseJson(
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
    )
  );
  // A build of another version (build-installer.mjs --version) names its
  // SBOMs after that version.
  const version = versionOverride ?? metadata.packages.find((p) => p.name === "nail-app")?.version;
  if (version === undefined) throw new Error("nail-app is not in cargo metadata");
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
    lock: /** @type {{ packages: Record<string, import("./lib/sbom.mjs").LockEntry> }} */ (
      readJson(path.join(repoRoot, "package-lock.json"))
    ),
    workspace: "ui",
    product: { name: "Nail UI", version },
    serialNumber: `urn:uuid:${randomUUID()}`,
    timestamp: timestamp(),
  });
  const uiFile = path.join(outDir, names.sbomUi);
  writeFileSync(uiFile, JSON.stringify(ui, null, 2) + "\n");
  return { rustFile, uiFile, crates: rust.components.length, npm: ui.components.length };
}

await runMain(import.meta.url, (argv) => {
  /** @type {{ outDir?: string, target?: string }} */
  const options = {};
  forEachArg(argv, (arg, { value }) => {
    if (arg === "--out") options.outDir = value();
    else if (arg === "--target") options.target = value();
    else throw new Error(`unknown argument ${arg}`);
  });
  const outDir = options.outDir ?? path.join(buildBase(process.env, repoRoot), "dist");
  const result = writeSboms({ outDir, target: options.target });
  console.log(
    `SBOM: ${result.rustFile} (${String(result.crates)} crates), ${result.uiFile} (${String(result.npm)} packages)`,
  );
});
