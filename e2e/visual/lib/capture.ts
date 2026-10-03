/**
 * Capturing a settled screen and recording what happened to it, for the
 * review page (`scripts/visual-review/`). Dependency-free (only erasable
 * TypeScript) so `node --test` runs `capture.test.ts` directly.
 */
import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";

export interface StableCapture {
  png: Buffer;
  /** Screenshots taken until two consecutive ones were identical. */
  attempts: number;
  /** False when the screen never settled within the attempts. */
  stable: boolean;
}

export interface StableOptions {
  /** Maximum screenshots (default 8). */
  attempts?: number;
  /** Pause between screenshots in ms (default 150). */
  intervalMs?: number;
  /** Waits between screenshots (injectable for tests). */
  sleep?: (ms: number) => Promise<void>;
}

const defaultSleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

/**
 * Takes screenshots until two consecutive ones are byte-identical (late
 * images, fonts, the reader frame and data arriving over the WebSocket all
 * settle) and answers the last one.
 */
export async function captureStable(
  shoot: () => Promise<Buffer>,
  options: StableOptions = {},
): Promise<StableCapture> {
  const attempts = Math.max(2, options.attempts ?? 8);
  const sleep = options.sleep ?? defaultSleep;
  let previous = await shoot();
  for (let i = 2; i <= attempts; i++) {
    await sleep(options.intervalMs ?? 150);
    const next = await shoot();
    if (next.equals(previous)) return { png: next, attempts: i, stable: true };
    previous = next;
  }
  return { png: previous, attempts, stable: false };
}

/**
 * What happened to one screen of one variant:
 * - `passed`: matches its baseline within the tolerance;
 * - `changed`: differs from its baseline (diff image available);
 * - `new`: there is no baseline yet;
 * - `captured`: captured without comparison (the harness smoke run);
 * - `skipped`: the variant cannot be captured (theme unavailable);
 * - `error`: the screen could not be opened or never settled.
 */
export type CaptureStatus = "passed" | "changed" | "new" | "captured" | "skipped" | "error";

/** One record of the manifest the review page is built from. */
export interface ManifestEntry {
  variant: string;
  theme: string;
  density: string;
  /** Screen id; `null` for a skipped variant (it has no screens). */
  screen: string | null;
  status: CaptureStatus;
  /** Absolute paths; `null` when there is no such image. */
  baseline: string | null;
  actual: string | null;
  diff: string | null;
  /** Why it was skipped / failed, or the comparison's message. */
  message: string | null;
}

/** The manifest file of an entry inside `dir` (one file per entry, so workers never collide). */
export function manifestFile(
  dir: string,
  entry: Pick<ManifestEntry, "variant" | "screen">,
): string {
  const safe = (s: string) => s.replace(/[^A-Za-z0-9._-]/g, "_");
  return path.join(dir, `${safe(entry.variant)}.${safe(entry.screen ?? "_variant")}.json`);
}

/** Writes an entry into the manifest directory; answers the file. */
export function writeManifestEntry(dir: string, entry: ManifestEntry): string {
  mkdirSync(dir, { recursive: true });
  const file = manifestFile(dir, entry);
  writeFileSync(file, `${JSON.stringify(entry, null, 2)}\n`);
  return file;
}

/** Status of a compared screen from the comparison outcome. */
export function comparisonStatus(baselineExists: boolean, matched: boolean): CaptureStatus {
  if (!baselineExists) return "new";
  return matched ? "passed" : "changed";
}
