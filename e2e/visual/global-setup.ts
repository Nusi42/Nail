/**
 * Starts every visual run with an empty manifest and no stale captures,
 * so the review page shows exactly this run. The baselines are untouched.
 */
import { rmSync } from "node:fs";
import { actualDir, manifestDir } from "./lib/paths.ts";

export default function globalSetup(): void {
  for (const dir of [manifestDir(), actualDir()]) {
    rmSync(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
  }
}
