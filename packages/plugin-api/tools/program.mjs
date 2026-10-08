/** Running a tool module as a command-line program (`nail-plugin`, `create-nail-plugin`). */
import { realpathSync } from "node:fs";
import { fileURLToPath } from "node:url";

/**
 * Whether the module at `moduleUrl` is the program (also through npm's bin
 * links, which are symlinks or wrappers).
 *
 * @param {string} moduleUrl the module's `import.meta.url`
 */
export function isProgram(moduleUrl) {
  try {
    return realpathSync(process.argv[1] ?? "") === realpathSync(fileURLToPath(moduleUrl));
  } catch {
    return false;
  }
}

/**
 * Runs `main` with the program's arguments when the module at `moduleUrl`
 * is the program, and sets the exit code from its answer.
 *
 * @param {string} moduleUrl the module's `import.meta.url`
 * @param {(argv: string[]) => number | Promise<number>} main
 */
export async function runAsProgram(moduleUrl, main) {
  if (isProgram(moduleUrl)) process.exitCode = await main(process.argv.slice(2));
}
