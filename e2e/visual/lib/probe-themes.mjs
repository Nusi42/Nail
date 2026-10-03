#!/usr/bin/env node
/**
 * Asks a throw-away `nail serve --seed demo-multi` which themes exist
 * (`ListThemes`, contract ADR 0102) and prints the raw answer as one JSON
 * line on stdout: `{"answer": <Response>}`, or `{"error": "<why>"}` when
 * the server could not be started or reached.
 *
 * The visual config runs this synchronously while Playwright collects the
 * tests, so the matrix follows the core's theme list (lib/matrix.ts).
 *
 * Usage: node probe-themes.mjs --bin <nail> --ui-dir <dir>
 * The server binds 127.0.0.1 on a free port (`--port 0`); its temporary
 * data and config directories are removed afterwards.
 */
import { spawn } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";

export function parseArgs(argv) {
  const options = { bin: "", uiDir: "", timeoutMs: 60_000 };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === "--bin") options.bin = argv[++i] ?? "";
    else if (arg === "--ui-dir") options.uiDir = argv[++i] ?? "";
    else if (arg === "--timeout-ms") options.timeoutMs = Number(argv[++i]);
    else throw new Error(`unknown argument ${arg}`);
  }
  if (!options.bin) throw new Error("--bin is required");
  if (!options.uiDir) throw new Error("--ui-dir is required");
  if (!Number.isFinite(options.timeoutMs) || options.timeoutMs <= 0) {
    throw new Error("--timeout-ms must be a positive number");
  }
  return options;
}

/** Sends one request over the server's WebSocket and resolves with the response. */
export function rpc(baseUrl, request, timeoutMs) {
  const url = new URL("api/ws", baseUrl);
  url.protocol = url.protocol.replace("http", "ws");
  const socket = new WebSocket(url);
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      socket.close();
      reject(new Error(`no answer to ${request.type}`));
    }, timeoutMs);
    socket.addEventListener("open", () => socket.send(JSON.stringify({ id: 1, request })));
    socket.addEventListener("message", (message) => {
      const frame = JSON.parse(String(message.data));
      if (frame.kind === "response" && frame.id === 1 && frame.response) {
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

async function probe(options) {
  const dirs = [
    mkdtempSync(path.join(tmpdir(), "nail-visual-probe-data-")),
    mkdtempSync(path.join(tmpdir(), "nail-visual-probe-config-")),
  ];
  const args = [
    "serve",
    "--port",
    "0",
    "--seed",
    "demo-multi",
    "--data-dir",
    dirs[0],
    "--config-dir",
    dirs[1],
    "--ui-dir",
    options.uiDir,
  ];
  const child = spawn(options.bin, args, {
    stdio: ["ignore", "pipe", "ignore"],
    env: { ...process.env, NAIL_LOG: process.env.NAIL_LOG ?? "error" },
  });
  try {
    const url = await new Promise((resolve, reject) => {
      const lines = createInterface({ input: child.stdout });
      const timer = setTimeout(
        () => reject(new Error("nail serve did not start")),
        options.timeoutMs,
      );
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
        reject(new Error(`nail serve exited with ${code}`));
      });
    });
    return { answer: await rpc(url, { type: "ListThemes" }, options.timeoutMs) };
  } finally {
    if (child.exitCode === null && child.pid !== undefined) {
      const exited = new Promise((resolve) => child.once("exit", resolve));
      child.kill();
      await exited;
    }
    for (const dir of dirs) {
      try {
        rmSync(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
      } catch {
        // Best effort: the OS cleans the temp directory eventually.
      }
    }
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  let result;
  try {
    result = await probe(parseArgs(process.argv.slice(2)));
  } catch (error) {
    result = { error: error instanceof Error ? error.message : String(error) };
  }
  process.stdout.write(`${JSON.stringify(result)}\n`);
}
