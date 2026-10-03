/**
 * Where the visual harness writes (never into the repository): the output
 * root `<build dir>/visual/` (`NAIL_VISUAL_RESULTS`, else `NAIL_BUILD_DIR`
 * or `%LOCALAPPDATA%\nail-build`), with `manifest/`, `actual/`,
 * `test-results/`, `report/` and the review page in `review/`.
 * scripts/visual-review/generate.mjs uses the same defaults.
 */
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

/** The checked-in baselines (`NAIL_VISUAL_BASELINES` overrides), per platform below it. */
export function baselinesRoot(env: NodeJS.ProcessEnv = process.env): string {
  if (env.NAIL_VISUAL_BASELINES) return path.resolve(env.NAIL_VISUAL_BASELINES);
  return path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "baselines");
}

/** The base build directory, as `e2e/support/nail.ts` `buildOutputDir` computes it. */
export function buildBase(env: NodeJS.ProcessEnv = process.env): string {
  if (env.NAIL_BUILD_DIR) return env.NAIL_BUILD_DIR;
  return env.LOCALAPPDATA
    ? path.join(env.LOCALAPPDATA, "nail-build")
    : path.join(tmpdir(), "nail-build");
}

/** The harness's output root. */
export function resultsDir(env: NodeJS.ProcessEnv = process.env): string {
  return env.NAIL_VISUAL_RESULTS ?? path.join(buildBase(env), "visual");
}

/** Where the manifest entries go. */
export const manifestDir = (env: NodeJS.ProcessEnv = process.env) =>
  path.join(resultsDir(env), "manifest");

/** Where the captured screenshots go (`<variant>/<screen>.png`). */
export const actualDir = (env: NodeJS.ProcessEnv = process.env) =>
  path.join(resultsDir(env), "actual");

/** Viewport of every screenshot (CSS px, device scale 1). */
export const VIEWPORT = { width: 1280, height: 800 } as const;

/**
 * The comparison tolerance: the share of pixels that may differ
 * (`NAIL_VISUAL_MAX_DIFF_RATIO`, default 0.002) and the per-pixel colour
 * distance 0–1 below which a pixel counts as equal
 * (`NAIL_VISUAL_THRESHOLD`, default 0.15; Playwright's comparator).
 */
export function tolerance(env: NodeJS.ProcessEnv = process.env) {
  const number = (value: string | undefined, fallback: number, max: number) => {
    const n = Number(value);
    return value !== undefined && value.trim() !== "" && Number.isFinite(n) && n >= 0 && n <= max
      ? n
      : fallback;
  };
  return {
    maxDiffPixelRatio: number(env.NAIL_VISUAL_MAX_DIFF_RATIO, 0.002, 1),
    threshold: number(env.NAIL_VISUAL_THRESHOLD, 0.15, 1),
  };
}
