/**
 * Starting the desktop app (Tauri + WebView2) for the smoke tests: the
 * binary built by `scripts/build-desktop.mjs`, launched with WebView2's
 * remote debugging port so Playwright can attach over CDP.
 */
import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import { existsSync } from "node:fs";
import path from "node:path";
import { chromium, expect, type Browser, type Page } from "@playwright/test";
import { buildOutputDir, repoRoot } from "./nail";

/** The desktop build's output directory (`<build dir>/desktop-target/debug`). */
function desktopOutput(file: string): string {
  const bin = path.join(buildOutputDir("desktop-target"), "debug", file);
  if (!existsSync(bin)) {
    throw new Error(`${bin} does not exist; run \`npm run build:desktop\` (or \`npm run e2e\`)`);
  }
  return bin;
}

/** Path of the desktop app: `$NAIL_APP_BIN`, else the desktop build's target dir. */
export function desktopBinary(): string {
  return process.env.NAIL_APP_BIN ?? desktopOutput("nail-app.exe");
}

/** The Simple MAPI DLL (`nail_mapi.dll`, shipped as `mapi\<arch>\NailMapi.dll`). */
export function mapiDll(): string {
  return desktopOutput("nail_mapi.dll");
}

/** The Simple MAPI test harness (calls the DLL like "Send to → Mail recipient"). */
export function mapiHarness(): string {
  return desktopOutput("nail-mapi-harness.exe");
}

/** The stand-in installer the updater scenario serves (records its command line). */
export function fakeInstaller(): string {
  return desktopOutput("nail-fake-installer.exe");
}

export interface DesktopApp {
  /** The CDP endpoint (`http://127.0.0.1:<port>`). */
  cdpUrl: string;
  /** The app's process id. */
  pid: number;
  /** Whether the app process has ended. */
  hasExited(): boolean;
  stop(): Promise<void>;
}

export interface DesktopOptions {
  /** What Windows would pass after `--` (a `mailto:` link, a file). */
  targets?: string[];
  /** More options before the targets (`--autostart`, `--config-dir DIR`, …). */
  args?: string[];
  /** More environment variables for the app (test variables of the shell). */
  env?: Record<string, string>;
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
 * profile unless `--data-dir` / `--config-dir` are given; ADR 0021) with
 * remote debugging on `port`, only for this process: the variable is set
 * in the child's environment. `options` may also be just the targets.
 */
export async function startDesktop(
  port: number,
  options: string[] | DesktopOptions = [],
): Promise<DesktopApp> {
  const {
    targets = [],
    args = [],
    env = {},
  } = Array.isArray(options) ? { targets: options } : options;
  const argv = ["--seed", "demo", ...args, ...(targets.length > 0 ? ["--", ...targets] : [])];
  const child = spawn(desktopBinary(), argv, {
    cwd: repoRoot,
    stdio: ["ignore", "ignore", "inherit"],
    env: {
      ...process.env,
      NAIL_LOG: process.env.NAIL_LOG ?? "warn",
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`,
      ...env,
    },
  });
  const cdpUrl = `http://127.0.0.1:${port}`;
  const exited = new Promise((resolve) => child.once("exit", resolve));
  const stop = async () => {
    if (child.exitCode !== null || child.pid === undefined) return;
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
  return {
    cdpUrl,
    pid: child.pid ?? 0,
    hasExited: () => child.exitCode !== null || child.signalCode !== null,
    stop,
  };
}

/** Connects Playwright to the app's WebView2 and returns its page. */
export async function connectDesktop(app: DesktopApp): Promise<{ browser: Browser; page: Page }> {
  const browser = await chromium.connectOverCDP(app.cdpUrl);
  await expect
    .poll(() => browser.contexts().flatMap((c) => c.pages()).length, { timeout: 60_000 })
    .toBeGreaterThan(0);
  const page = browser.contexts().flatMap((c) => c.pages())[0]!;
  // The built UI from the custom protocol, loaded (also in a hidden window).
  await page.waitForURL(/^(tauri:\/\/localhost|https?:\/\/tauri\.localhost)/, {
    timeout: 60_000,
  });
  await page.waitForFunction(
    () => document.readyState === "complete" && "__TAURI_INTERNALS__" in window,
    undefined,
    { timeout: 60_000 },
  );
  return { browser, page };
}

/**
 * Closes the first-run screen with "Skip" (M4b: the desktop app offers it
 * once per session until completed; a seeded run starts fresh).
 */
export async function skipFirstRun(page: Page): Promise<void> {
  const welcome = page.getByRole("dialog", { name: "Welcome to Nail" });
  await expect(welcome).toBeVisible({ timeout: 60_000 });
  await welcome.getByRole("button", { name: "Skip" }).click();
  await expect(welcome).toBeHidden();
}

/**
 * The shell's test control of seeded runs (`nail_test_shell`, never in a
 * real profile): a tray menu item id (`tray-new-message`, …) runs that
 * entry; `window-visible` answers whether the main window shows.
 */
export async function shellControl(page: Page, op: string): Promise<unknown> {
  return page.evaluate(async (op) => {
    const internals = (
      window as unknown as {
        __TAURI_INTERNALS__: { invoke(cmd: string, args: unknown): Promise<unknown> };
      }
    ).__TAURI_INTERNALS__;
    return internals.invoke("nail_test_shell", { op });
  }, op);
}
