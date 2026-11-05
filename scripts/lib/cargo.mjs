/**
 * Binaries of the Cargo workspace, found one way (wave Q2, DRY) by the e2e
 * harness (`nail` debug), the performance run (`nail`, `nail-bench`
 * release) and their scripts: an environment variable names the binary, else
 * it is in cargo's target directory (`cargo metadata` respects
 * `CARGO_TARGET_DIR`). Tests in scripts/test/cargo.test.mjs.
 *
 * Also the number of parallel compile jobs (ADR 0214): chosen from the free
 * memory when a build starts, unless `CARGO_BUILD_JOBS` is set.
 */
import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import os from "node:os";
import path from "node:path";

import { parseJson } from "./json.mjs";
import { probe } from "./probe.mjs";

/** Bytes in a GiB. */
export const GIB = 1024 ** 3;

/** Memory one compile job may take (a large crate or a test binary's link). */
export const GIB_PER_JOB = 3;
/** Memory left for everything else while cargo runs. */
export const RESERVED_GIB = 4;
/** The fewest jobs, however little memory is free. */
export const MIN_JOBS = 2;

/**
 * Compile jobs for `availableGib` free memory and `cpus` hardware threads:
 * one per {@link GIB_PER_JOB} after {@link RESERVED_GIB}, at least
 * {@link MIN_JOBS}, at most `cpus`.
 *
 * @param {{ availableGib: number, cpus: number }} machine
 * @returns {number}
 */
export function cargoJobs({ availableGib, cpus }) {
  const byMemory = Math.floor((availableGib - RESERVED_GIB) / GIB_PER_JOB);
  return Math.max(MIN_JOBS, Math.min(Math.max(cpus, MIN_JOBS), byMemory));
}

/**
 * Free memory in GiB. Windows: the smaller of free physical memory and the
 * free commit charge (a full paging file fails rustc with os error 1455).
 * macOS counts the file cache as used, so half of the total memory is
 * taken when more is free than that suggests. Linux: `MemAvailable`.
 *
 * @param {{ platform?: NodeJS.Platform, freemem?: number, totalmem?: number,
 *   freeCommit?: () => number | null }} [probe] for tests; the real machine otherwise
 * @returns {number}
 */
export function availableGib(probe = {}) {
  const platform = probe.platform ?? process.platform;
  const free = probe.freemem ?? os.freemem();
  const total = probe.totalmem ?? os.totalmem();
  if (platform === "win32") {
    const commit = (probe.freeCommit ?? windowsFreeCommit)();
    return Math.min(free, commit ?? free) / GIB;
  }
  if (platform === "darwin") return Math.max(free, total / 2) / GIB;
  return free / GIB;
}

/** Windows' free commit charge in bytes, or null when it cannot be read. */
function windowsFreeCommit() {
  const kib = Number(
    probe("powershell", [
      "-NoProfile",
      "-NonInteractive",
      "-Command",
      "(Get-CimInstance Win32_OperatingSystem).FreeVirtualMemory",
    ]),
  );
  return Number.isFinite(kib) && kib > 0 ? kib * 1024 : null;
}

/**
 * Sets `CARGO_BUILD_JOBS` in `env` from the free memory, unless it is set
 * already (an explicit choice wins), and returns it. Cargo and every child
 * process started with `env` use it.
 *
 * @param {Partial<Record<string, string>>} [env]
 * @param {() => number} [available] free memory in GiB
 * @returns {string}
 */
export function ensureCargoJobs(env = process.env, available = availableGib) {
  const set = env.CARGO_BUILD_JOBS;
  if (set) return set;
  const gib = available();
  const jobs = String(cargoJobs({ availableGib: gib, cpus: os.availableParallelism() }));
  env.CARGO_BUILD_JOBS = jobs;
  console.log(`cargo: ${jobs} jobs (${gib.toFixed(1)} GiB free; CARGO_BUILD_JOBS overrides)`);
  return jobs;
}

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
