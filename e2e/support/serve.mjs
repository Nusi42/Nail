/**
 * Starting `nail serve`, one way (wave Q2, DRY) for the e2e harness
 * (support/nail.ts), the performance scenarios (perf/support.ts) and the
 * visual harness's theme probe (visual/lib/probe-themes.mjs): the server
 * prints its URL as the first line on stdout; logs go to stderr
 * (`NAIL_LOG`, default `error`). `coreRequest` sends it one request.
 */
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";

/**
 * A running `nail serve`.
 *
 * @typedef {object} ServeProcess
 * @property {string} url the URL it printed
 * @property {() => Promise<void>} stop ends it and waits for the exit
 */

/**
 * @typedef {object} ServeOptions
 * @property {number} timeoutMs how long to wait for the URL
 * @property {string} [cwd] the working directory
 * @property {"inherit" | "ignore"} [stderr] where its logs go (default `inherit`)
 */

/**
 * Runs `<binary> …args` and waits for the URL it prints first.
 *
 * @param {string} binary the `nail` binary
 * @param {readonly string[]} args `serve` and its options
 * @param {ServeOptions} options
 * @returns {Promise<ServeProcess>}
 */
export async function spawnServe(binary, args, options) {
  const child = spawn(binary, args, {
    cwd: options.cwd,
    stdio: ["ignore", "pipe", options.stderr ?? "inherit"],
    env: { ...process.env, NAIL_LOG: process.env.NAIL_LOG ?? "error" },
  });
  const stop = async () => {
    if (child.exitCode === null && child.pid !== undefined) {
      const exited = new Promise((resolve) => {
        child.once("exit", resolve);
      });
      child.kill();
      await exited;
    }
  };
  try {
    /** @type {string} */
    const url = await new Promise((resolve, reject) => {
      const lines = createInterface({ input: child.stdout });
      const timer = setTimeout(() => {
        reject(new Error("nail serve did not start"));
      }, options.timeoutMs);
      lines.once("line", (line) => {
        clearTimeout(timer);
        resolve(line.trim());
      });
      child.once("error", (error) => {
        clearTimeout(timer);
        reject(error);
      });
      child.once("exit", (code) => {
        clearTimeout(timer);
        reject(new Error(`nail serve exited with ${String(code)}`));
      });
    });
    return { url, stop };
  } catch (error) {
    await stop();
    throw error;
  }
}

/**
 * Sends one request to the core over the server's WebSocket (what the UI
 * sends) and resolves with its answer.
 *
 * @param {string} baseUrl
 * @param {object} request
 * @param {number} timeoutMs
 * @returns {Promise<unknown>}
 */
export function coreRequest(baseUrl, request, timeoutMs) {
  const url = new URL("api/ws", baseUrl);
  url.protocol = url.protocol.replace("http", "ws");
  const socket = new WebSocket(url);
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      socket.close();
      reject(new Error(`no answer to ${JSON.stringify(request)}`));
    }, timeoutMs);
    socket.addEventListener("open", () => {
      socket.send(JSON.stringify({ id: 1, request }));
    });
    socket.addEventListener("message", (message) => {
      /** @type {unknown} */
      const frame = JSON.parse(String(message.data));
      if (
        typeof frame === "object" &&
        frame !== null &&
        "kind" in frame &&
        frame.kind === "response" &&
        "id" in frame &&
        frame.id === 1 &&
        "response" in frame &&
        frame.response
      ) {
        clearTimeout(timer);
        socket.close();
        resolve(frame.response);
      }
    });
    socket.addEventListener("error", () => {
      clearTimeout(timer);
      reject(new Error("WebSocket error"));
    });
  });
}
