/**
 * Binaries of the Cargo workspace, found one way (wave Q2, DRY) by the e2e
 * harness (`nail` debug), the performance run (`nail`, `nail-bench`
 * release) and their scripts: an environment variable names the binary, else
 * it is in cargo's target directory (`cargo metadata` respects
 * `CARGO_TARGET_DIR`). Tests in scripts/test/cargo.test.mjs.
 */
import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import path from "node:path";

import { parseJson } from "./json.mjs";

/**
 * Cargo's target directory of the workspace at `repoRoot`.
 *
 * @param {string} repoRoot
 * @returns {string}
 */
export function cargoTargetDir(repoRoot) {
  const metadata = /** @type {{ target_directory: string }} */ (
    parseJson(
      execFileSync("cargo", ["metadata", "--format-version", "1", "--no-deps"], {
        cwd: repoRoot,
        encoding: "utf8",
      }),
    )
  );
  return metadata.target_directory;
}

/**
 * @typedef {object} BinarySpec
 * @property {string} repoRoot the workspace
 * @property {string} name the binary (`nail`, `nail-bench`)
 * @property {string} pkg its package, for the build hint (`nail-cli`)
 * @property {"debug" | "release"} profile
 * @property {string} envName the variable that names the binary instead (`NAIL_BIN`)
 * @property {Partial<Record<string, string>>} [env] the environment (`process.env`)
 * @property {string} [targetDir] cargo's target directory (asked when left out)
 */

/**
 * A binary of the workspace: `$<envName>`, else the built one in cargo's
 * target directory. Throws with the build command when it is missing.
 *
 * @param {BinarySpec} spec
 * @returns {string}
 */
export function workspaceBinary(spec) {
  const env = spec.env ?? process.env;
  const fromEnv = env[spec.envName];
  if (fromEnv) return fromEnv;
  const exe = process.platform === "win32" ? `${spec.name}.exe` : spec.name;
  const targetDir = spec.targetDir ?? cargoTargetDir(spec.repoRoot);
  const bin = path.join(targetDir, spec.profile, exe);
  if (!existsSync(bin)) {
    const release = spec.profile === "release" ? " --release" : "";
    throw new Error(`${bin} does not exist; run \`cargo build${release} -p ${spec.pkg}\` first`);
  }
  return bin;
}
