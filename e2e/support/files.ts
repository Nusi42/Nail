import { readFileSync } from "node:fs";

/**
 * A text file's content, or "" while it does not exist. One read, no
 * `existsSync` first: the core may remove the file (an undo) between a
 * check and the read.
 */
export function readTextOrEmpty(file: string): string {
  try {
    return readFileSync(file, "utf8");
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return "";
    throw error;
  }
}
