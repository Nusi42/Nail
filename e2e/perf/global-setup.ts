/**
 * Makes sure the benchmark data exists before the perf scenarios: reuses
 * the last `nail-bench run` of the same corpus in the work directory, or
 * runs it (`NAIL_PERF_FRESH=1` always runs it). The Rust metrics of that
 * run land in `report-rust.json`; the scenarios add `report-ui.json`.
 */
import { execFileSync } from "node:child_process";
import {
  budgetsPath,
  corpus,
  corpusMessages,
  readRustReport,
  releaseBinary,
  repoRoot,
  workDir,
} from "./support";

export default function globalSetup(): void {
  const report = readRustReport();
  const wanted = corpusMessages[corpus];
  if (wanted === undefined) throw new Error(`unknown NAIL_PERF_CORPUS ${corpus}`);
  const fresh = process.env.NAIL_PERF_FRESH === "1";
  if (!fresh && report && report.corpus.messages === wanted) return;
  execFileSync(
    releaseBinary("nail-bench"),
    ["run", "--corpus", corpus, "--work-dir", workDir(), "--budgets", budgetsPath, "--no-check"],
    { cwd: repoRoot, stdio: "inherit" },
  );
}
