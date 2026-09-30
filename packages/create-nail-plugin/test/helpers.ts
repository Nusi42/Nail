/**
 * Helpers of the scaffolder and example tests: plug-in folders in temp
 * directories whose `node_modules` link to this repository's SDK, esbuild
 * and TypeScript (no `npm install`, no network), and the built bundle
 * loaded against the SDK's test host.
 */
import { execFileSync } from "node:child_process";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import type { DefinedPlugin, PluginManifest } from "@nail/plugin-api";
import { createTestHost, type TestHost, type TestHostOptions } from "@nail/plugin-api/testing";

const here = path.dirname(fileURLToPath(import.meta.url));
/** The repository root. */
export const REPO = path.resolve(here, "..", "..", "..");
/** `packages/plugin-api`. */
export const SDK = path.join(REPO, "packages", "plugin-api");

const require = createRequire(import.meta.url);

/** The real folder of an installed package. */
function packageDir(name: string): string {
  return path.dirname(require.resolve(`${name}/package.json`));
}

/** A fresh temp directory (removed by the returned cleanup). */
export function tempDir(prefix: string): { dir: string; cleanup: () => void } {
  const dir = mkdtempSync(path.join(tmpdir(), prefix));
  return { dir, cleanup: () => rmSync(dir, { recursive: true, force: true }) };
}

/**
 * Links `<dir>/node_modules/{@nail/plugin-api,esbuild,typescript}` to this
 * repository's copies (directory junctions on Windows: no admin rights).
 */
export function linkDependencies(dir: string): void {
  const nm = path.join(dir, "node_modules");
  mkdirSync(path.join(nm, "@nail"), { recursive: true });
  const link = (target: string, at: string) => symlinkSync(target, at, "junction");
  link(SDK, path.join(nm, "@nail", "plugin-api"));
  link(packageDir("esbuild"), path.join(nm, "esbuild"));
  link(packageDir("typescript"), path.join(nm, "typescript"));
}

/** Copies a plug-in folder without `dist/` and `node_modules/`. */
export function copyPlugin(from: string, to: string): void {
  cpSync(from, to, {
    recursive: true,
    filter: (src) => {
      const rel = path.relative(from, src).split(path.sep)[0];
      return rel !== "dist" && rel !== "node_modules";
    },
  });
}

/** Runs `node <script> …args` in `cwd`; returns stdout (throws with the output on failure). */
export function node(cwd: string, script: string, ...args: string[]): string {
  try {
    return execFileSync(process.execPath, [script, ...args], {
      cwd,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
    });
  } catch (error) {
    const e = error as { stdout?: string; stderr?: string; message: string };
    throw new Error(`${script} failed: ${e.message}\n${e.stdout ?? ""}\n${e.stderr ?? ""}`, {
      cause: error,
    });
  }
}

/** Type-checks a plug-in folder with its tsconfig.json. */
export function typecheck(dir: string): string {
  return node(dir, path.join(packageDir("typescript"), "bin", "tsc"), "-p", ".");
}

/** Reads `<dir>/manifest.json`. */
export function readManifest(dir: string): PluginManifest {
  return JSON.parse(readFileSync(path.join(dir, "manifest.json"), "utf8")) as PluginManifest;
}

/** A loaded bundle with its test host. */
export interface LoadedPlugin {
  host: TestHost;
  plugin: DefinedPlugin;
  restore: () => void;
}

let loads = 0;

/**
 * Imports a built bundle (an ES module) with a test host installed as
 * `globalThis.nail`, and activates it like Nail does.
 */
export async function loadBundle(file: string, options: TestHostOptions): Promise<LoadedPlugin> {
  const host = createTestHost(options);
  const restore = host.install();
  try {
    loads += 1;
    const mod = (await import(`${pathToFileURL(file).href}?load=${loads}`)) as {
      default: DefinedPlugin;
    };
    await mod.default.activate(host.activationContext());
    return { host, plugin: mod.default, restore };
  } catch (error) {
    restore();
    throw error;
  }
}
