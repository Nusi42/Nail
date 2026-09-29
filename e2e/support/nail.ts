/**
 * Locating and starting the prebuilt `nail` binary.
 *
 * The e2e suite never builds anything: `npm run build -w ui` and
 * `cargo build -p nail-cli` must have run first (the root `npm run e2e`
 * script does both).
 */
import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";

export const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");

/** Path of the `nail` binary: `$NAIL_BIN`, else cargo's debug target dir. */
export function nailBinary(): string {
  const fromEnv = process.env.NAIL_BIN;
  if (fromEnv) return fromEnv;
  const metadata = JSON.parse(
    execFileSync("cargo", ["metadata", "--format-version", "1", "--no-deps"], {
      cwd: repoRoot,
      encoding: "utf8",
    }),
  ) as { target_directory: string };
  const exe = process.platform === "win32" ? "nail.exe" : "nail";
  const bin = path.join(metadata.target_directory, "debug", exe);
  if (!existsSync(bin)) {
    throw new Error(`${bin} does not exist; run \`cargo build -p nail-cli\` first`);
  }
  return bin;
}

/** A fresh temporary directory. */
export function tempDir(prefix: string): string {
  return mkdtempSync(path.join(tmpdir(), `nail-e2e-${prefix}-`));
}

export interface NailServer {
  url: string;
  dataDir: string;
  configDir: string;
  stop(): Promise<void>;
}

export interface StartOptions {
  locale?: string;
  /** Demo data: the local corpus (default) or the in-process IMAP server. */
  seed?: "demo" | "demo-imap";
}

/**
 * Starts a private `nail serve --seed …` on a free port with its own data
 * and config directories (for tests that change the configuration).
 */
export async function startNail(options: StartOptions = {}): Promise<NailServer> {
  const dataDir = tempDir("data");
  const configDir = tempDir("config");
  const args = [
    "serve",
    "--port",
    "0",
    "--seed",
    options.seed ?? "demo",
    "--data-dir",
    dataDir,
    "--config-dir",
    configDir,
  ];
  if (options.locale) args.push("--locale", options.locale);
  const child: ChildProcess = spawn(nailBinary(), args, {
    cwd: repoRoot,
    stdio: ["ignore", "pipe", "inherit"],
    env: { ...process.env, NAIL_LOG: process.env.NAIL_LOG ?? "error" },
  });
  const url = await new Promise<string>((resolve, reject) => {
    const lines = createInterface({ input: child.stdout! });
    const timer = setTimeout(() => reject(new Error("nail serve did not start")), 30_000);
    lines.once("line", (line) => {
      clearTimeout(timer);
      resolve(line.trim());
    });
    child.once("exit", (code) => {
      clearTimeout(timer);
      reject(new Error(`nail serve exited with ${code}`));
    });
  });
  return {
    url,
    dataDir,
    configDir,
    async stop() {
      if (child.exitCode === null) {
        const exited = new Promise((resolve) => child.once("exit", resolve));
        child.kill();
        await exited;
      }
      for (const dir of [dataDir, configDir]) {
        rmSync(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
      }
    },
  };
}
