/**
 * Development ports without collisions (plan section 10, "Development
 * ports"): the Vite dev server for `npm run dev` / `npm run tauri dev`
 * takes a free port instead of a fixed one, so Nail runs next to the
 * owner's other Vite / Tauri projects.
 *
 * - `NAIL_DEV_PORT` names the port exactly; a taken one is an error.
 * - Otherwise the preferred port ({@link PREFERRED_DEV_PORT}, deliberately
 *   not Vite's 5173) is used when free, else any free port the OS hands out.
 *
 * Everything binds 127.0.0.1 only (ADR 0057). Pure helpers here; the CLI is
 * scripts/dev.mjs, the tests scripts/test/dev-port.test.mjs.
 */
import net from "node:net";
import os from "node:os";
import path from "node:path";
import fs from "node:fs";

/** The dev server's preferred port: not Vite's default 5173. */
export const PREFERRED_DEV_PORT = 5791;

/** The loopback host the dev server binds and Tauri loads from. */
export const DEV_HOST = "127.0.0.1";

/** Environment variable that fixes the dev port. */
export const DEV_PORT_ENV = "NAIL_DEV_PORT";

/**
 * Parses a `NAIL_DEV_PORT` value: `undefined` when unset or empty, else a
 * port in 1–65535 (throws on anything else).
 */
export function parsePortEnv(value) {
  if (value === undefined || value.trim() === "") return undefined;
  const text = value.trim();
  const port = Number(text);
  if (!/^\d+$/.test(text) || !Number.isInteger(port) || port < 1 || port > 65535) {
    throw new Error(`${DEV_PORT_ENV} must be a port between 1 and 65535, not "${value}"`);
  }
  return port;
}

/** Binds `port` on {@link DEV_HOST} (0 = any free port) and returns the server. */
function bind(port) {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.unref();
    server.once("error", reject);
    server.listen({ port, host: "127.0.0.1", exclusive: true }, () => resolve(server));
  });
}

function close(server) {
  return new Promise((resolve) => server.close(() => resolve()));
}

/** Whether `port` can be bound on 127.0.0.1 right now. */
export async function isPortFree(port) {
  try {
    await close(await bind(port));
    return true;
  } catch {
    return false;
  }
}

/** A port the OS reports free on 127.0.0.1. */
export async function freePort() {
  const server = await bind(0);
  const { port } = server.address();
  await close(server);
  return port;
}

/**
 * Chooses the dev server port: `NAIL_DEV_PORT` exactly (taken = error),
 * else `preferred` when free, else a free port.
 *
 * @param {{env?: Record<string, string | undefined>, preferred?: number,
 *   isFree?: (port: number) => Promise<boolean>, anyFree?: () => Promise<number>}} options
 * @returns {Promise<{port: number, source: "env" | "preferred" | "free"}>}
 */
export async function pickDevPort({
  env = process.env,
  preferred = PREFERRED_DEV_PORT,
  isFree = isPortFree,
  anyFree = freePort,
} = {}) {
  const fixed = parsePortEnv(env[DEV_PORT_ENV]);
  if (fixed !== undefined) {
    if (!(await isFree(fixed))) {
      throw new Error(`${DEV_PORT_ENV}=${fixed} is taken; free it or choose another port`);
    }
    return { port: fixed, source: "env" };
  }
  if (await isFree(preferred)) return { port: preferred, source: "preferred" };
  return { port: await anyFree(), source: "free" };
}

/** The dev server URL Tauri loads for `port`. */
export function devUrl(port) {
  return `http://${DEV_HOST}:${port}`;
}

/**
 * The `tauri dev --config` override: load the UI from our Vite server and
 * start no dev server of Tauri's own (`null` removes `beforeDevCommand` in
 * the JSON merge patch the Tauri CLI applies).
 */
export function tauriDevConfig(port) {
  return { build: { devUrl: devUrl(port), beforeDevCommand: null } };
}

/**
 * Writes the override to a temporary file (outside the repository) and
 * returns its path; the Tauri CLI accepts a path for `--config`, which
 * avoids quoting JSON through the Windows shell.
 */
export function writeTauriDevConfig(port, dir = os.tmpdir()) {
  const file = path.join(dir, `nail-tauri-dev-${process.pid}.json`);
  fs.writeFileSync(file, JSON.stringify(tauriDevConfig(port), null, 2));
  return file;
}

/**
 * Splits the dev script's arguments: `tauri` as the first argument runs
 * the desktop app too; everything after it goes to `tauri dev`.
 */
export function parseDevArgs(argv) {
  if (argv[0] === "tauri") return { tauri: true, tauriArgs: argv.slice(1) };
  return { tauri: false, tauriArgs: [] };
}
