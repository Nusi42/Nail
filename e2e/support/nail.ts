/**
 * Locating and starting the prebuilt `nail` binary.
 *
 * The e2e suite never builds anything: `npm run build -w ui` and
 * `cargo build -p nail-cli` must have run first (the root `npm run e2e`
 * script does both).
 */
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { test } from "@playwright/test";
import { buildBase } from "../../scripts/lib/build.mjs";
import { workspaceBinary } from "../../scripts/lib/cargo.mjs";
import { repoRoot } from "../../scripts/lib/program.mjs";
import { spawnServe, type ServeProcess } from "./serve.mjs";

export { repoRoot, spawnServe, type ServeProcess };

/** Path of the `nail` binary: `$NAIL_BIN`, else cargo's debug target dir. */
export function nailBinary(): string {
  return workspaceBinary({
    repoRoot,
    name: "nail",
    pkg: "nail-cli",
    profile: "debug",
    envName: "NAIL_BIN",
  });
}

/**
 * A free TCP port on 127.0.0.1, found synchronously (the Playwright config is
 * evaluated synchronously) by letting a child process bind port 0.
 */
export function freePort(): number {
  const script =
    "const s=require('node:net').createServer();" +
    "s.listen(0,'127.0.0.1',()=>{process.stdout.write(String(s.address().port));s.close();});";
  const port = Number(execFileSync(process.execPath, ["-e", script], { encoding: "utf8" }));
  if (!Number.isInteger(port) || port <= 0) throw new Error(`no free port: ${String(port)}`);
  return port;
}

/**
 * A directory for build and test output: `<checkout>/.nail-build/<name>`
 * (gitignored; each work folder has its own). `NAIL_BUILD_DIR` overrides the
 * base.
 */
export function buildOutputDir(name: string): string {
  return path.join(buildBase(process.env, repoRoot), name);
}

/**
 * `<build dir>/screens/<folder>/<name>.png` for a test's screenshot (the
 * directory is created; characters unsafe in file names become `-`).
 */
export function screenshotFile(folder: string, name: string): string {
  const dir = buildOutputDir(path.join("screens", folder));
  mkdirSync(dir, { recursive: true });
  return path.join(dir, `${name.replace(/[^\w.-]+/g, "-")}.png`);
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
  /**
   * Demo data: the local corpus (default), the in-process IMAP server, or
   * two configured accounts on mock IMAP / POP3 / SMTP servers
   * (`demo-multi`, with the test controls of {@link control}).
   */
  seed?: "demo" | "demo-imap" | "demo-multi";
  /** Files / links to open, as Windows hands them to the desktop app (`--launch`). */
  launch?: string[];
  /**
   * Existing data and config directories (a restart of an earlier run,
   * M4c); they are not deleted by `stop()`. Seeded runs keep their secrets
   * in `<dataDir>/demo-keychain.json`, so a restart finds them again.
   */
  dirs?: { dataDir: string; configDir: string };
  /**
   * The seeded run's "now" (ADR 0185, `NAIL_DEMO_NOW`): the demo-multi
   * calendar content is dated from its day and the core's clock starts at
   * it. Default: the real time.
   */
  demoNow?: Date;
}

/**
 * Starts a private `nail serve --seed …` on a free port with its own data
 * and config directories (for tests that change the configuration).
 */
export async function startNail(options: StartOptions = {}): Promise<NailServer> {
  const dataDir = options.dirs?.dataDir ?? tempDir("data");
  const configDir = options.dirs?.configDir ?? tempDir("config");
  const keepDirs = options.dirs !== undefined;
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
    // The freshly built bundle, never a stale `ui` folder beside the binary
    // (Tauri copies its resources into the target directory).
    "--ui-dir",
    path.join(repoRoot, "ui", "dist"),
  ];
  if (options.locale) args.push("--locale", options.locale);
  for (const target of options.launch ?? []) args.push("--launch", target);
  const serve = await spawnServe(nailBinary(), args, {
    cwd: repoRoot,
    timeoutMs: 30_000,
    env: options.demoNow ? { NAIL_DEMO_NOW: options.demoNow.toISOString() } : {},
  });
  return {
    url: serve.url,
    dataDir,
    configDir,
    async stop() {
      await serve.stop();
      if (keepDirs) return;
      for (const dir of [dataDir, configDir]) {
        rmSync(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
      }
    },
  };
}

/** Prepares a started server before the tests use it. */
type Ready = (server: NailServer) => void | Promise<void>;

/**
 * One server slot of a spec file: `start` / `stop` run `nail serve`,
 * `view` is a `NailServer` whose fields and `stop()` reach the running
 * one (an error while none runs).
 */
function serverSlot() {
  let current: NailServer | undefined;
  const running = (): NailServer => {
    if (!current) throw new Error("no nail serve is running (used outside a test?)");
    return current;
  };
  const view: NailServer = {
    get url() {
      return running().url;
    },
    get dataDir() {
      return running().dataDir;
    },
    get configDir() {
      return running().configDir;
    },
    stop: () => running().stop(),
  };
  const start = async (options: StartOptions, ready?: Ready): Promise<NailServer> => {
    current = await startNail(options);
    if (ready !== undefined) await ready(current);
    return current;
  };
  const stop = async (): Promise<void> => {
    // The start may have failed before a server ran.
    await current?.stop();
    current = undefined;
  };
  return { view, start, stop };
}

/** The server of a spec file's lifecycle hooks (`each` test or `all` of them). */
function hookedServer(scope: "each" | "all", options: StartOptions, ready?: Ready): NailServer {
  const slot = serverSlot();
  const start = async () => {
    await slot.start(options, ready);
  };
  if (scope === "each") {
    test.beforeEach(start);
    test.afterEach(slot.stop);
  } else {
    test.beforeAll(start);
    test.afterAll(slot.stop);
  }
  return slot.view;
}

/** A `nail serve` that each test starts itself (`start`, with its own options). */
export interface OnDemandServer extends NailServer {
  start(options?: StartOptions): Promise<NailServer>;
}

/**
 * A server each test of the calling spec file starts when it needs one
 * (with that test's options); it is stopped after the test.
 */
export function nailOnDemand(): OnDemandServer {
  const slot = serverSlot();
  test.afterEach(slot.stop);
  const view = slot.view;
  return {
    get url() {
      return view.url;
    },
    get dataDir() {
      return view.dataDir;
    },
    get configDir() {
      return view.configDir;
    },
    stop: () => view.stop(),
    start: (options: StartOptions = {}) => slot.start(options),
  };
}

/**
 * A fresh `nail serve` for each test of the calling spec file (or
 * `describe` block); `ready` prepares it before the test.
 */
export function nailPerTest(options: StartOptions = {}, ready?: Ready): NailServer {
  return hookedServer("each", options, ready);
}

/**
 * One `nail serve` for every test of the calling spec file (or `describe`
 * block); `ready` prepares it before the first test.
 */
export function nailPerFile(options: StartOptions = {}, ready?: Ready): NailServer {
  return hookedServer("all", options, ready);
}

/**
 * The calling spec file's `test` with a fixture `nail`: a fresh
 * `nail serve` per test that asks for it.
 */
export function nailFixture(options: StartOptions = {}) {
  return test.extend<{ nail: NailServer }>({
    // eslint-disable-next-line no-empty-pattern -- Playwright requires a destructuring pattern for fixtures without dependencies
    nail: async ({}, use) => {
      const server = await startNail(options);
      await use(server);
      await server.stop();
    },
  });
}

/**
 * Runs a test-control command of a `--seed demo-multi` server (it drives
 * the in-process mock servers: deliver mail, change flags on the server,
 * go offline, read what the SMTP server received). See
 * `nail_core::demo::DemoMulti::control` for the commands.
 */
export async function control<T = unknown>(
  server: NailServer,
  command: string,
  args: Record<string, unknown> = {},
): Promise<T> {
  const response = await fetch(new URL(`api/test/${command}`, server.url), {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(args),
  });
  if (!response.ok) {
    throw new Error(
      `control ${command} failed: ${String(response.status)} ${await response.text()}`,
    );
  }
  return (await response.json()) as T;
}

/** A message on a mock IMAP server, as `imap-messages` lists it. */
export interface ServerMessage {
  uid: number;
  subject: string;
  flags: string[];
}

/** The messages of a mailbox on the demo IMAP server (`work` by default). */
export function serverMessages(
  server: NailServer,
  mailbox: string,
  account = "work",
): Promise<ServerMessage[]> {
  return control<ServerMessage[]>(server, "imap-messages", { account, mailbox });
}
