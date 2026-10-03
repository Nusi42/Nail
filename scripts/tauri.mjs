#!/usr/bin/env node
/**
 * `npm run tauri <command> [args…]`: the Tauri CLI, except that `dev` goes
 * through scripts/dev.mjs (a free dev-server port and the matching devUrl
 * instead of a fixed port; plan section 10).
 */
import { spawn } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const [command, ...rest] = process.argv.slice(2);

const args =
  command === "dev"
    ? [path.join(root, "scripts", "dev.mjs"), "tauri", ...rest]
    : [path.join(root, "node_modules", "@tauri-apps", "cli", "tauri.js"), ...process.argv.slice(2)];
const child = spawn(process.execPath, args, { cwd: process.cwd(), stdio: "inherit" });
child.on("exit", (code, signal) => process.exit(code ?? (signal ? 1 : 0)));
