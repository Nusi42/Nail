#!/usr/bin/env node
/**
 * Development server without fixed ports (plan section 10).
 *
 *   node scripts/dev.mjs                 # Vite dev server (npm run dev)
 *   node scripts/dev.mjs tauri [args…]   # Vite + `tauri dev [args…]` (npm run tauri dev)
 *
 * The port comes from scripts/lib/dev-port.mjs: NAIL_DEV_PORT exactly, else
 * 5791 when free, else any free port. Vite runs on exactly that port
 * (strictPort) on 127.0.0.1; `tauri dev` gets a `--config` override with
 * the matching `build.devUrl` and no `beforeDevCommand`, so Tauri neither
 * starts a second dev server nor waits on a fixed port.
 */
import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  DEV_HOST,
  DEV_PORT_ENV,
  devUrl,
  parseDevArgs,
  pickDevPort,
  writeTauriDevConfig,
} from "./lib/dev-port.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const uiDir = path.join(root, "ui");

async function main() {
  const { tauri, tauriArgs } = parseDevArgs(process.argv.slice(2));
  const { port, source } = await pickDevPort();
  // vite.config.ts reads the port from the environment too, so a reload of
  // the config keeps it.
  process.env[DEV_PORT_ENV] = String(port);

  const { createServer } = await import("vite");
  const vite = await createServer({
    root: uiDir,
    configFile: path.join(uiDir, "vite.config.ts"),
    server: { host: DEV_HOST, port, strictPort: true },
  });
  await vite.listen();
  const how =
    source === "env" ? `from ${DEV_PORT_ENV}` : source === "free" ? "5791 was taken" : "preferred";
  console.log(`nail dev server: ${devUrl(port)}/ (${how})`);

  if (!tauri) {
    vite.bindCLIShortcuts?.({ print: true });
    return;
  }

  const config = writeTauriDevConfig(port);
  const cli = path.join(root, "node_modules", "@tauri-apps", "cli", "tauri.js");
  const child = spawn(process.execPath, [cli, "dev", "--config", config, ...tauriArgs], {
    cwd: root,
    stdio: "inherit",
  });
  const stop = async (code) => {
    try {
      fs.rmSync(config, { force: true });
    } catch {
      // A leftover temp file is harmless.
    }
    await vite.close();
    process.exit(code);
  };
  child.on("exit", (code, signal) => stop(code ?? (signal ? 1 : 0)));
  child.on("error", (err) => {
    console.error(err);
    stop(1);
  });
}

main().catch((err) => {
  console.error(err instanceof Error ? err.message : err);
  process.exit(1);
});
