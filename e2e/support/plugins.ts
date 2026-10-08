/**
 * Plug-in packages for the plug-in scenarios (ADRs 0055, 0084): the
 * examples built from `examples/<name>` (esbuild + `@nail/plugin-api`'s
 * packer, output outside the repository), and small hand-written plug-ins
 * packed or laid out as development folders.
 */
import { spawn, type ChildProcess } from "node:child_process";
import { cpSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import { createInterface } from "node:readline";
import { packPlugin, pluginBuildOptions } from "@nail/plugin-api/tools";
import { exampleNames } from "../../packages/create-nail-plugin/scaffold.mjs";
import { expect, type Locator, type Page } from "@playwright/test";
import * as esbuild from "esbuild";
import { buildOutputDir, nailBinary, repoRoot, tempDir } from "./nail";
import { closeWindow, openSettingsTab } from "./views";

/** The examples the global setup builds (left in `NAIL_E2E_EXAMPLE_<NAME>`). */
export { exampleNames };
export type ExampleName = string;

/** The environment variable holding an example's package path. */
export function exampleEnv(name: ExampleName): string {
  return `NAIL_E2E_EXAMPLE_${name.toUpperCase().replace(/-/g, "_")}`;
}

/**
 * Builds `examples/<name>` into `<build>/e2e/plugins/<name>` with the
 * examples' own build (`nail-plugin build`'s esbuild settings) and packs it;
 * returns the `.nailplugin` path. Nothing is written into the repo.
 */
export async function buildExample(name: ExampleName): Promise<string> {
  const example = path.join(repoRoot, "examples", name);
  const staging = path.join(buildOutputDir("e2e"), "plugins", name);
  rmSync(staging, { recursive: true, force: true });
  mkdirSync(staging, { recursive: true });
  cpSync(path.join(example, "manifest.json"), path.join(staging, "manifest.json"));
  cpSync(path.join(example, "locales"), path.join(staging, "locales"), { recursive: true });
  await esbuild.build({
    ...pluginBuildOptions(example, { outfile: path.join(staging, "dist", "index.js") }),
    logLevel: "warning",
  });
  const out = path.join(staging, "..", `${name}.nailplugin`);
  return packPlugin({ dir: staging, out }).file;
}

/** An example's package (built by the global setup). */
export function examplePackage(name: ExampleName): string {
  const file = process.env[exampleEnv(name)];
  if (!file) throw new Error(`${exampleEnv(name)} is not set; the global setup did not run`);
  return file;
}

/** The "Follow up" example's package. */
export function followUpPackage(): string {
  return examplePackage("follow-up");
}

/**
 * A plug-in written as files: `manifest` becomes manifest.json, `main`
 * index.js, `files` more files by relative path (catalogs).
 */
export interface PluginFiles {
  manifest: Record<string, unknown>;
  main: string;
  files?: Record<string, string>;
}

/** Writes a plug-in folder (a development folder); returns its path. */
export function writePluginFolder(files: PluginFiles, dir = tempDir("plugin")): string {
  writeFileSync(path.join(dir, "manifest.json"), JSON.stringify(files.manifest, null, 2));
  writeFileSync(path.join(dir, "index.js"), files.main);
  for (const [relative, text] of Object.entries(files.files ?? {})) {
    const file = path.join(dir, relative);
    mkdirSync(path.dirname(file), { recursive: true });
    writeFileSync(file, text);
  }
  return dir;
}

/** Packs a plug-in written as files; returns the `.nailplugin` path. */
export function packFiles(files: PluginFiles): string {
  const dir = writePluginFolder(files);
  return packPlugin({ dir, out: path.join(dir, "..", `${path.basename(dir)}.nailplugin`) }).file;
}

/** A manifest with one command placed in the reader toolbar after "Forward". */
export function commandManifest(
  id: string,
  name: string,
  command: { id: string; title: string },
  permissions: string[],
  version = "1.0.0",
): Record<string, unknown> {
  return {
    id,
    name,
    version,
    apiVersion: "1",
    main: "index.js",
    permissions,
    contributes: {
      commands: [{ ...command, contexts: ["reader"] }],
      placements: [{ toolbar: "reader", after: "mail.forward", command: command.id }],
    },
  };
}

/**
 * Installs the package `file` through Settings → Plug-ins: checks it (its
 * heading matches `title`), installs it and, with `place`, adds its
 * contributions to the layout. Answers the settings view.
 */
export async function installPlugin(
  page: Page,
  file: string,
  title: RegExp,
  options: { place?: boolean } = {},
): Promise<Locator> {
  const settings = await openSettingsTab(page, "Plug-ins");
  await settings.getByRole("button", { name: "Install a plug-in…" }).click();
  const dialog = page.getByRole("dialog", { name: "Install a plug-in" });
  await dialog.getByRole("textbox", { name: "Path of the .nailplugin file" }).fill(file);
  await dialog.getByRole("button", { name: "Check the plug-in" }).click();
  await expect(dialog.getByRole("heading", { name: title })).toBeVisible();
  await dialog.getByRole("button", { name: "Install", exact: true }).click();
  await expect(dialog.getByText(/is installed\./)).toBeVisible();
  if (options.place === true) {
    await dialog.getByRole("button", { name: /^Add \d+ items? to the layout$/ }).click();
    await expect(dialog.getByText(/Added to your layout/)).toBeVisible();
  }
  await closeWindow(dialog);
  return settings;
}

/** A running `nail plugin dev` console: its stdout lines. */
export interface DevConsole {
  child: ChildProcess;
  lines: string[];
  /** Waits for a console line containing `text`. */
  waitFor(text: string): Promise<void>;
}

/**
 * Runs `nail plugin dev <folder> …target` (`--url URL` of a running
 * `nail serve`, or `--data-dir DIR` of the desktop app) in English.
 */
export function pluginDevConsole(folder: string, target: readonly string[]): DevConsole {
  const child = spawn(nailBinary(), ["plugin", "dev", folder, ...target], {
    stdio: ["ignore", "pipe", "inherit"],
    env: { ...process.env, NAIL_LOG: "error", NAIL_LOCALE: "en" },
  });
  const lines: string[] = [];
  createInterface({ input: child.stdout }).on("line", (line) => lines.push(line));
  return {
    child,
    lines,
    async waitFor(text: string) {
      await expect
        .poll(() => lines.some((l) => l.includes(text)), {
          message: `console line with "${text}"; saw ${JSON.stringify(lines)}`,
          timeout: 30_000,
        })
        .toBe(true);
    },
  };
}
