/**
 * Starting the desktop app (Tauri + WebView2) for the smoke test: the
 * binary built by `scripts/build-desktop.mjs`, launched with WebView2's
 * remote debugging port so Playwright can attach over CDP.
 */
import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import { existsSync } from "node:fs";
import path from "node:path";
import { buildOutputDir, repoRoot } from "./nail";

/** Path of the desktop app: `$NAIL_APP_BIN`, else the desktop build's target dir. */
export function desktopBinary(): string {
  const fromEnv = process.env.NAIL_APP_BIN;
  if (fromEnv) return fromEnv;
  const bin = path.join(buildOutputDir("desktop-target"), "debug", "nail-app.exe");
  if (!existsSync(bin)) {
    throw new Error(`${bin} does not exist; run \`npm run build:desktop\` (or \`npm run e2e\`)`);
  }
  return bin;
}

export interface DesktopApp {
  /** The CDP endpoint (`http://127.0.0.1:<port>`). */
  cdpUrl: string;
  stop(): Promise<void>;
}

/** Waits until the WebView2 DevTools endpoint answers. */
async function waitForCdp(url: string, child: ChildProcess, timeoutMs: number): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (child.exitCode !== null) throw new Error(`nail-app exited with ${child.exitCode}`);
    try {
      const response = await fetch(`${url}/json/version`);
      if (response.ok) return;
    } catch {
      // Not listening yet.
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error(`WebView2 did not open its debugging port at ${url}`);
}

/**
 * Starts `nail-app --seed demo` (temporary data, config and WebView2
 * profile; ADR 0021) with remote debugging on `port`, only for this
 * process: the variable is set in the child's environment.
 */
export async function startDesktop(port: number): Promise<DesktopApp> {
  const child = spawn(desktopBinary(), ["--seed", "demo"], {
    cwd: repoRoot,
    stdio: ["ignore", "ignore", "inherit"],
    env: {
      ...process.env,
      NAIL_LOG: process.env.NAIL_LOG ?? "warn",
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`,
    },
  });
  const cdpUrl = `http://127.0.0.1:${port}`;
  const stop = async () => {
    if (child.exitCode !== null || child.pid === undefined) return;
    const exited = new Promise((resolve) => child.once("exit", resolve));
    try {
      // The WebView2 browser processes are children of the app.
      execFileSync("taskkill", ["/pid", String(child.pid), "/T", "/F"], { stdio: "ignore" });
    } catch {
      child.kill();
    }
    await exited;
  };
  try {
    await waitForCdp(cdpUrl, child, 90_000);
  } catch (error) {
    await stop();
    throw error;
  }
  return { cdpUrl, stop };
}
