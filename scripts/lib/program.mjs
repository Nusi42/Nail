/**
 * Node scripts run as programs, one way (wave Q2, DRY): `isMain` tells
 * whether a module is the program node started, `runMain` runs its `main`
 * then and turns the answer into the exit code (an error prints its message
 * and exits with 2). `repoRoot` is the checkout the scripts belong to.
 * Tests in scripts/test/program.test.mjs.
 */
import path from "node:path";
import { fileURLToPath } from "node:url";

/** The checkout's root directory. */
export const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");

/** The exit code of a program whose `main` threw. */
export const ERROR_EXIT_CODE = 2;

/**
 * Whether the module at `moduleUrl` is the program node started.
 *
 * @param {string} moduleUrl the module's `import.meta.url`
 * @param {readonly string[]} argv the process arguments (`process.argv`)
 */
export function isMain(moduleUrl, argv = process.argv) {
  const program = argv[1];
  return program !== undefined && path.resolve(program) === fileURLToPath(moduleUrl);
}

/**
 * Runs `main` with the program's arguments when the module at `moduleUrl`
 * is the program. A number it answers becomes the exit code; an error is
 * printed and exits with `ERROR_EXIT_CODE`.
 *
 * @param {string} moduleUrl the module's `import.meta.url`
 * @param {(argv: string[]) => number | void | Promise<number | void>} main
 * @param {readonly string[]} argv the process arguments (`process.argv`)
 * @returns {Promise<void>}
 */
export async function runMain(moduleUrl, main, argv = process.argv) {
  if (!isMain(moduleUrl, argv)) return;
  try {
    const status = await main(argv.slice(2));
    if (typeof status === "number") process.exitCode = status;
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = ERROR_EXIT_CODE;
  }
}
