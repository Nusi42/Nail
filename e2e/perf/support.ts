/**
 * Helpers of the performance scenarios (plan M4c WP123, ADR 0094): where
 * the benchmark data lives, the release binaries, a `nail serve` on that
 * data, the budgets and the UI report that `nail-bench check` merges with
 * the Rust report.
 *
 * The data is what `nail-bench run` left in its work directory (a synced
 * corpus of `NAIL_PERF_CORPUS`, default `standard` = 100,000 messages):
 * `<build dir>/perf/<corpus>/{data,config,report-rust.json}`. Nothing goes
 * into the repository.
 */
import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { createInterface } from "node:readline";
import { buildOutputDir, repoRoot } from "../support/nail";
import { defined } from "../support/defined";

export { repoRoot };

/** The corpus the scenarios run on (`standard` or `small`). */
export const corpus = process.env.NAIL_PERF_CORPUS ?? "standard";

/** Messages of the named corpora (`nail_bench::CorpusSpec`). */
export const corpusMessages: Record<string, number> = { standard: 100_000, small: 2_000 };

/** The benchmark's work directory. */
export function workDir(): string {
  return process.env.NAIL_PERF_WORK ?? buildOutputDir(path.join("perf", corpus));
}

/** The budgets file. */
export const budgetsPath = path.join(repoRoot, "e2e", "perf", "budgets.json");

/** A folder as the Rust report names it. */
export interface FolderFact {
  id: string;
  path: string;
  total: number;
}

/** The parts of `report-rust.json` the scenarios use. */
export interface RustReport {
  corpus: { messages: number };
  facts: {
    "data-dir": string;
    "config-dir": string;
    messages: number;
    "largest-folder"?: FolderFact | null;
  };
}

/** The Rust report of the last `nail-bench run`, if any. */
export function readRustReport(): RustReport | null {
  const file = path.join(workDir(), "report-rust.json");
  if (!existsSync(file)) return null;
  return JSON.parse(readFileSync(file, "utf8")) as RustReport;
}

/**
 * A release binary of the workspace (`nail`, `nail-bench`): `$NAIL_BIN` /
 * `$NAIL_BENCH_BIN`, else cargo's release directory (`CARGO_TARGET_DIR`
 * respected through `cargo metadata`). UI timings need the optimized core.
 */
export function releaseBinary(name: "nail" | "nail-bench"): string {
  const fromEnv = name === "nail" ? process.env.NAIL_BIN : process.env.NAIL_BENCH_BIN;
  if (fromEnv) return fromEnv;
  const metadata = JSON.parse(
    execFileSync("cargo", ["metadata", "--format-version", "1", "--no-deps"], {
      cwd: repoRoot,
      encoding: "utf8",
    }),
  ) as { target_directory: string };
  const exe = process.platform === "win32" ? `${name}.exe` : name;
  const bin = path.join(metadata.target_directory, "release", exe);
  if (!existsSync(bin)) {
    const pkg = name === "nail" ? "nail-cli" : "nail-bench";
    throw new Error(`${bin} does not exist; run \`cargo build --release -p ${pkg}\` first`);
  }
  return bin;
}

export interface PerfServer {
  url: string;
  stop(): Promise<void>;
}

/** `nail serve` on the benchmark data (no seed, no MCP server). */
export async function startServe(dataDir: string, configDir: string): Promise<PerfServer> {
  const args = [
    "serve",
    "--port",
    "0",
    "--no-mcp",
    "--data-dir",
    dataDir,
    "--config-dir",
    configDir,
    "--ui-dir",
    path.join(repoRoot, "ui", "dist"),
  ];
  const child: ChildProcess = spawn(releaseBinary("nail"), args, {
    cwd: repoRoot,
    stdio: ["ignore", "pipe", "inherit"],
    env: { ...process.env, NAIL_LOG: process.env.NAIL_LOG ?? "error" },
  });
  const url = await new Promise<string>((resolve, reject) => {
    const lines = createInterface({ input: defined(child.stdout, "the server's stdout") });
    const timer = setTimeout(() => {
      reject(new Error("nail serve did not start"));
    }, 120_000);
    lines.once("line", (line) => {
      clearTimeout(timer);
      resolve(line.trim());
    });
    child.once("exit", (code) => {
      clearTimeout(timer);
      reject(new Error(`nail serve exited with ${String(code)}`));
    });
  });
  return {
    url,
    async stop() {
      if (child.exitCode === null) {
        const exited = new Promise((resolve) => {
          child.once("exit", resolve);
        });
        child.kill();
        await exited;
      }
    },
  };
}

/** The `p`-th percentile (nearest rank), like `nail_bench::run::percentile`. */
export function percentile(values: number[], p: number): number {
  if (values.length === 0) return 0;
  const sorted = [...values].sort((a, b) => a - b);
  const rank = Math.ceil((p / 100) * sorted.length);
  return sorted[Math.min(Math.max(rank, 1), sorted.length) - 1] ?? 0;
}

/** The median. */
export function median(values: number[]): number {
  return percentile(values, 50);
}

interface BudgetsFile {
  "ci-tolerance"?: number;
  budgets: { metric: string; max: number }[];
}

/**
 * The effective maximum of a metric: the budget, times the CI tolerance
 * when `NAIL_PERF_TOLERANT=1` (the CI job; shared runners are slower).
 */
export function budgetMax(metric: string): number {
  const file = JSON.parse(readFileSync(budgetsPath, "utf8")) as BudgetsFile;
  const budget = file.budgets.find((b) => b.metric === metric);
  if (!budget) throw new Error(`no budget for ${metric}`);
  const tolerant = process.env.NAIL_PERF_TOLERANT === "1";
  return budget.max * (tolerant ? Math.max(1, file["ci-tolerance"] ?? 1) : 1);
}

/** One measurement, in `nail_bench::Metric`'s shape. */
export interface Metric {
  metric: string;
  value: number;
  samples: number;
}

/** Writes the UI report next to the Rust one; answers its path. */
export function writeUiReport(metrics: Metric[], details: Record<string, unknown>): string {
  const dir = workDir();
  mkdirSync(dir, { recursive: true });
  const file = path.join(dir, "report-ui.json");
  writeFileSync(file, JSON.stringify({ corpus, metrics, details }, null, 2));
  return file;
}

/** A trace event as Chrome writes it. */
export interface TraceEvent {
  name?: string;
  ph?: string;
  ts?: number;
  dur?: number;
  pid?: number;
  tid?: number;
  args?: { name?: string };
}

/**
 * Main-thread tasks of the renderer longer than `limitMs` in a Chrome
 * trace (`devtools.timeline`): the long frames' causes.
 */
export function longRendererTasks(traceJson: string, limitMs = 50): number[] {
  const parsed = JSON.parse(traceJson) as { traceEvents?: TraceEvent[] } | TraceEvent[];
  const events = Array.isArray(parsed) ? parsed : (parsed.traceEvents ?? []);
  const mainThreads = new Set(
    events
      .filter((e) => e.ph === "M" && e.name === "thread_name" && e.args?.name === "CrRendererMain")
      .map((e) => `${String(e.pid)}:${String(e.tid)}`),
  );
  return events
    .filter(
      (e) =>
        e.ph === "X" &&
        (e.name === "RunTask" || e.name === "ThreadControllerImpl::RunTask") &&
        mainThreads.has(`${String(e.pid)}:${String(e.tid)}`) &&
        (e.dur ?? 0) > limitMs * 1000,
    )
    .map((e) => (e.dur ?? 0) / 1000);
}
