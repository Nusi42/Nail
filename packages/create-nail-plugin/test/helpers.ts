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
import { loadCatalogs, readZip } from "@nail/plugin-api/tools";
import { exampleNames } from "../scaffold.mjs";
import { afterAll, afterEach, beforeAll, expect } from "vitest";

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
  return {
    dir,
    cleanup: () => {
      rmSync(dir, { recursive: true, force: true });
    },
  };
}

/**
 * Links `<dir>/node_modules/{@nail/plugin-api,esbuild,typescript}` to this
 * repository's copies (directory junctions on Windows: no admin rights).
 */
export function linkDependencies(dir: string): void {
  const nm = path.join(dir, "node_modules");
  mkdirSync(path.join(nm, "@nail"), { recursive: true });
  const link = (target: string, at: string) => {
    symlinkSync(target, at, "junction");
  };
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

/**
 * Runs `nail-plugin …args` (the linked SDK's CLI, as npm's bin link would)
 * in `dir`; returns stdout.
 */
export function nailPlugin(dir: string, ...args: string[]): string {
  return node(
    dir,
    path.join(dir, "node_modules", "@nail", "plugin-api", "tools", "cli.mjs"),
    ...args,
  );
}

/**
 * Builds and packs the plug-in `id` in `dir` with `nail-plugin` (the build
 * prints nothing on stdout); returns the archive's entry names.
 */
export function buildAndPack(dir: string, id: string): string[] {
  expect(nailPlugin(dir, "build")).toBe("");
  expect(nailPlugin(dir, "pack")).toMatch(new RegExp(`${id}\\.nailplugin`));
  return readZip(readFileSync(path.join(dir, "dist", `${id}.nailplugin`))).map((e) => e.name);
}

/** Type-checks a plug-in folder with its tsconfig.json. */
export function typecheck(dir: string): string {
  return node(dir, path.join(packageDir("typescript"), "bin", "tsc"), "-p", ".");
}

/** Reads `<dir>/manifest.json`. */
export function readManifest(dir: string): PluginManifest {
  return JSON.parse(readFileSync(path.join(dir, "manifest.json"), "utf8")) as PluginManifest;
}

/** The example plug-ins in `examples/`. */
export { exampleNames };

/** An example copied to a temp folder, type-checked and built. */
export interface PreparedExample {
  /** The example in the repository. */
  source: string;
  /** The copy (with linked dependencies and `dist/index.js`). */
  dir: string;
  /** The built bundle. */
  bundle: string;
  /** Removes the copy. */
  cleanup: () => void;
}

/**
 * Copies `examples/<name>` to a temp folder (without `dist/` and
 * `node_modules/`), links the dependencies, type-checks and builds it
 * with its own scripts — nothing is written into the repository.
 */
export function prepareExample(name: string): PreparedExample {
  const source = path.join(REPO, "examples", name);
  const t = tempDir(`nail-${name}-`);
  const dir = path.join(t.dir, name);
  copyPlugin(source, dir);
  linkDependencies(dir);
  typecheck(dir);
  nailPlugin(dir, "build");
  return { source, dir, bundle: path.join(dir, "dist", "index.js"), cleanup: t.cleanup };
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
    const mod = (await import(`${pathToFileURL(file).href}?load=${String(loads)}`)) as {
      default: DefinedPlugin;
    };
    await mod.default.activate(host.activationContext());
    return { host, plugin: mod.default, restore };
  } catch (error) {
    restore();
    throw error;
  }
}

/** An example prepared for a test file, and one of its source modules. */
export interface ExampleUnderTest<M> {
  /** The prepared copy (after `beforeAll`). */
  readonly example: PreparedExample;
  /** `src/<module>.ts` of the example, imported (after `beforeAll`). */
  readonly module: M;
}

/**
 * Prepares `examples/<name>` for the calling test file (in `beforeAll`,
 * removed in `afterAll`) and imports its `src/<module>.ts` for unit tests
 * of the plug-in's own logic.
 */
export function useExample<M = never>(name: string, module?: string): ExampleUnderTest<M> {
  let example: PreparedExample | undefined;
  let imported: M | undefined;
  beforeAll(async () => {
    example = prepareExample(name);
    if (module !== undefined) {
      const file = path.join(example.source, "src", `${module}.ts`);
      imported = (await import(pathToFileURL(file).href)) as M;
    }
  });
  afterAll(() => {
    example?.cleanup();
  });
  const ready = <T>(value: T | undefined, what: string): T => {
    if (value === undefined) throw new Error(`${what} is not ready (used outside a test?)`);
    return value;
  };
  return {
    get example() {
      return ready(example, `the ${name} example`);
    },
    get module() {
      return ready(imported, `${name}/src/${String(module)}.ts`);
    },
  };
}

/**
 * The `load(options)` of a describe block: loads the plug-in built in
 * `dir()` (`dist/index.js`, its manifest and catalogs, then `defaults()`
 * and `options`) into a fresh test host, and restores the global `nail`
 * after each test.
 */
export function pluginLoader(
  dir: () => string,
  defaults: () => TestHostOptions = () => ({}),
): (options?: TestHostOptions) => Promise<TestHost> {
  let loaded: LoadedPlugin | undefined;
  afterEach(() => {
    loaded?.restore();
    loaded = undefined;
  });
  return async (options = {}) => {
    loaded?.restore();
    const root = dir();
    loaded = await loadBundle(path.join(root, "dist", "index.js"), {
      manifest: readManifest(root),
      catalogs: loadCatalogs(root),
      ...defaults(),
      ...options,
    });
    return loaded.host;
  };
}
