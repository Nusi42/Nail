/**
 * Starting the desktop app (Tauri + WebView2) for the smoke tests: the
 * binary built by `scripts/build-desktop.mjs`, launched with WebView2's
 * remote debugging port so Playwright can attach over CDP.
 */
import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import { existsSync, writeFileSync } from "node:fs";
import path from "node:path";
import { chromium, expect, test, type Browser, type Page } from "@playwright/test";
import { buildOutputDir, freePort, repoRoot, tempDir } from "./nail";
import { defined } from "./defined";

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
  /** {@link launchDesktop}: leave the first-run screen open. */
  keepFirstRun?: boolean;
}

/**
 * `NAIL_TEST_BACKGROUND_WINDOWS` for the app (ADR 0146): on, so its windows
 * are cloaked, never activated and off the taskbar — they never take the
 * developer's mouse or keyboard. Set it to `0` to watch the windows.
 */
const backgroundWindows = process.env.NAIL_TEST_BACKGROUND_WINDOWS ?? "1";

/** Waits until the WebView2 DevTools endpoint answers. */
async function waitForCdp(url: string, child: ChildProcess, timeoutMs: number): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (child.exitCode !== null) throw new Error(`nail-app exited with ${String(child.exitCode)}`);
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
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${String(port)}`,
      NAIL_TEST_BACKGROUND_WINDOWS: backgroundWindows,
      ...env,
    },
  });
  const cdpUrl = `http://127.0.0.1:${String(port)}`;
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
  const page = defined(browser.contexts().flatMap((c) => c.pages())[0], "the app's page");
  // The built UI from the custom protocol, loaded (also in a hidden window).
  await page.waitForURL(/^(tauri:\/\/localhost|https?:\/\/tauri\.localhost)/, {
    timeout: 60_000,
  });
  await page.waitForFunction(
    () => document.readyState === "complete" && "__TAURI_INTERNALS__" in window,
    undefined,
    { timeout: 60_000 },
  );
  await expectWindowsInBackground(page);
  return { browser, page };
}

/**
 * Marks the calling spec file (or `describe` block) as desktop tests: they
 * need Windows with WebView2 and `npm run build:desktop`, run in CI only
 * with `NAIL_E2E_DESKTOP=1` (locally always), one app window at a time,
 * each test within `timeout`.
 */
export function desktopTests(timeout = 240_000): void {
  test.skip(process.platform !== "win32", "the desktop app uses WebView2 (Windows)");
  test.skip(
    !!process.env.CI && !process.env.NAIL_E2E_DESKTOP,
    "set NAIL_E2E_DESKTOP=1 to run the desktop tests in CI",
  );
  test.describe.configure({ mode: "serial", timeout });
}

/** A desktop app the tests drive: the process, Playwright's connection and the app's page. */
export interface DesktopSession {
  app: DesktopApp;
  browser: Browser;
  page: Page;
}

/** Ends a session (also one only half started). */
async function closeSession(session: Partial<DesktopSession>): Promise<void> {
  await session.browser?.close().catch(() => undefined);
  await session.app?.stop();
}

/**
 * Starts the desktop app on a free port, connects to it and skips the
 * first-run screen. A start that fails (WebView2 slow to open its port or
 * to load the page on a busy machine — the P3 flake) is retried once with
 * a new app.
 */
export async function launchDesktop(
  options: string[] | DesktopOptions = [],
): Promise<DesktopSession> {
  let lastError: unknown;
  for (let attempt = 1; attempt <= 2; attempt++) {
    const session: Partial<DesktopSession> = {};
    try {
      session.app = await startDesktop(freePort(), options);
      const { browser, page } = await connectDesktop(session.app);
      session.browser = browser;
      if (Array.isArray(options) || options.keepFirstRun !== true) await skipFirstRun(page);
      return { app: session.app, browser, page };
    } catch (error) {
      lastError = error;
      await closeSession(session);
      console.warn(`desktop start ${String(attempt)} failed: ${String(error)}`);
    }
  }
  throw lastError;
}

/**
 * How long a file's shared app may take to start: two attempts of
 * {@link launchDesktop} (CDP port, page, first-run screen), beyond the
 * tests' own timeout.
 */
export const DESKTOP_START_TIMEOUT = 600_000;

/**
 * One desktop app for every test of the calling spec file (or `describe`
 * block), started in `beforeAll` (with its own timeout) and stopped in
 * `afterAll`; its fields reach the running session.
 */
export function desktopPerFile(options: string[] | DesktopOptions = []): DesktopSession {
  let current: DesktopSession | undefined;
  const running = (): DesktopSession => {
    if (!current) throw new Error("no desktop app is running (used outside a test?)");
    return current;
  };
  test.beforeAll(async () => {
    test.setTimeout(DESKTOP_START_TIMEOUT);
    current = await launchDesktop(options);
  });
  test.afterAll(async () => {
    // `beforeAll` may have failed before the app ran.
    if (current) await closeSession(current);
    current = undefined;
  });
  return {
    get app() {
      return running().app;
    },
    get browser() {
      return running().browser;
    },
    get page() {
      return running().page;
    },
  };
}

/**
 * A desktop app per test of the calling spec file: the answer launches one
 * ({@link launchDesktop}); every app a test launched is stopped after it.
 */
export function desktopPerTest(): (options?: string[] | DesktopOptions) => Promise<DesktopSession> {
  const sessions: DesktopSession[] = [];
  test.afterEach(async () => {
    for (const session of sessions.splice(0)) await closeSession(session);
  });
  return async (options = []) => {
    const session = await launchDesktop(options);
    sessions.push(session);
    return session;
  };
}

/**
 * A config directory with one switched-off account and Ada's identity:
 * the composer can send (to the outbox) without a mail server.
 */
export function configWithIdentity(): string {
  const dir = tempDir("desktop-config");
  writeFileSync(
    path.join(dir, "accounts.kdl"),
    [
      'account "work" email="ada@example.org" name="Work" enabled=#false {',
      '    imap host="127.0.0.1" port=1 tls="plaintext"',
      '    smtp host="127.0.0.1" port=1 tls="plaintext"',
      '    identity "ada@example.org" name="Ada Lovelace"',
      "}",
      "",
    ].join("\n"),
  );
  return dir;
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

/** One OS window of the app as `nail_test_shell "windows"` reports it. */
export interface ShellWindow {
  label: string;
  /** Shown as far as Windows is concerned. */
  visible: boolean;
  /** Cloaked by DWM: never drawn, no mouse input. */
  cloaked: boolean;
  /** Could be activated (no `WS_EX_NOACTIVATE`). */
  activatable: boolean;
  /** The foreground window. */
  foreground: boolean;
  /** Cloaked, not activatable and not in the foreground. */
  inBackground: boolean;
}

/** The answer of `nail_test_shell "windows"`. */
export interface ShellWindows {
  /** The app runs with background windows. */
  active: boolean;
  /** The foreground window belongs to the app (a window, a dialog). */
  foreground: boolean;
  windows: ShellWindow[];
}

/**
 * Checks that the app keeps its windows away from the developer (ADR
 * 0146): every OS window is cloaked, cannot be activated and is not in the
 * foreground, and the foreground window is not the app's. Answers the
 * report (with `NAIL_TEST_BACKGROUND_WINDOWS=0` only the report).
 */
export async function expectWindowsInBackground(page: Page): Promise<ShellWindows> {
  const report = (await shellControl(page, "windows")) as ShellWindows;
  if (backgroundWindows === "0") return report;
  expect(report.active, "the app runs with background windows").toBe(true);
  expect(report.windows.length, "the app's windows").toBeGreaterThan(0);
  expect(
    report.windows.filter((w) => !w.inBackground),
    "windows the developer could see, click or type into",
  ).toEqual([]);
  expect(report.foreground, "the app took the foreground").toBe(false);
  return report;
}
