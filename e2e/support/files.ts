import { readFileSync } from "node:fs";

/** Error codes of a file the core is replacing at this moment (Windows). */
const BUSY = new Set(["EPERM", "EBUSY", "EACCES"]);

/** Reads that may meet the file while it is being replaced. */
const BUSY_ATTEMPTS = 20;

/**
 * A text file's content, or "" while it does not exist. One read, no
 * `existsSync` first: the core may remove the file (an undo) between a
 * check and the read. On Windows a file the core is replacing (write to a
 * temporary file, then rename) cannot be opened for a moment: such a read
 * is tried again.
 */
export function readTextOrEmpty(file: string): string {
  for (let attempt = 1; ; attempt += 1) {
    try {
      return readFileSync(file, "utf8");
    } catch (error) {
      const code = (error as NodeJS.ErrnoException).code ?? "";
      if (code === "ENOENT") return "";
      if (!BUSY.has(code) || attempt >= BUSY_ATTEMPTS) throw error;
      // A short synchronous pause (the callers read synchronously).
      Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 25);
    }
  }
}
