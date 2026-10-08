/**
 * Where the visual harness writes (never into tracked files): the output
 * root `<build dir>/visual/` (`NAIL_VISUAL_RESULTS`, else `NAIL_BUILD_DIR`
 * or the checkout's gitignored `.nail-build`), with `manifest/`, `actual/`,
 * `test-results/`, `report/` and the review page in `review/`.
 * scripts/visual-review/generate.mjs uses the same defaults.
 */
import path from "node:path";
import { fileURLToPath } from "node:url";
import { buildBase as checkoutBuildBase } from "../../../scripts/lib/build.mjs";
import { repoRoot } from "../../../scripts/lib/program.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));

/** The checkout this harness belongs to. */
export { repoRoot };

/** The checked-in baselines (`NAIL_VISUAL_BASELINES` overrides), per platform below it. */
export function baselinesRoot(env: NodeJS.ProcessEnv = process.env): string {
  if (env.NAIL_VISUAL_BASELINES) return path.resolve(env.NAIL_VISUAL_BASELINES);
  return path.resolve(here, "..", "baselines");
}

/** The base build directory, as `e2e/support/nail.ts` `buildOutputDir` computes it. */
export function buildBase(env: NodeJS.ProcessEnv = process.env): string {
  return checkoutBuildBase(env, repoRoot);
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
// M5b: 1000 px high so the list under the list header shows the sample message.
export const VIEWPORT = { width: 1280, height: 1000 } as const;

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
