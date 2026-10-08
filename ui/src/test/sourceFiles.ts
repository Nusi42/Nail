/**
 * The UI's source files for tests that check rules over the code (i18n
 * coverage, the themeable-components rule, shared texts): every file below
 * `dir` whose name matches `include`, skipping the generated bindings and
 * the folders named in `skipDirs`.
 */
import { readdirSync } from "node:fs";
import { join, resolve } from "node:path";

/** `ui/src` of the checkout (Vitest runs in `ui/`). */
export const SRC_DIR = resolve(process.cwd(), "src");

export interface SourceFilter {
  /** File names to keep. */
  include: RegExp;
  /** File names to drop among those (e.g. tests). */
  exclude?: RegExp;
  /** Folder names to skip besides `generated`. */
  skipDirs?: readonly string[];
}

/** Absolute paths of the matching files, depth first. */
export function sourceFiles(dir: string, filter: SourceFilter): string[] {
  const skip = new Set(["generated", ...(filter.skipDirs ?? [])]);
  const out: string[] = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (!skip.has(entry.name)) out.push(...sourceFiles(path, filter));
    } else if (filter.include.test(entry.name) && !filter.exclude?.test(entry.name)) {
      out.push(path);
    }
  }
  return out;
}
