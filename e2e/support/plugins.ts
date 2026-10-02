/**
 * Plug-in packages for the plug-in scenarios (ADRs 0055, 0084): the
 * examples built from `examples/<name>` (esbuild + `@nail/plugin-api`'s
 * packer, output outside the repository), and small hand-written plug-ins
 * packed or laid out as development folders.
 */
import { cpSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import { packPlugin } from "@nail/plugin-api/tools";
import * as esbuild from "esbuild";
import { buildOutputDir, repoRoot, tempDir } from "./nail";

/** Where the global setup leaves the built examples (`NAIL_E2E_EXAMPLE_<NAME>`). */
export const EXAMPLES = ["follow-up", "attachment-check", "csv-preview"] as const;
export type ExampleName = (typeof EXAMPLES)[number];

/** The environment variable holding an example's package path. */
export function exampleEnv(name: ExampleName): string {
  return `NAIL_E2E_EXAMPLE_${name.toUpperCase().replace(/-/g, "_")}`;
}

/**
 * Builds `examples/<name>` into `<build>/e2e/plugins/<name>` with the
 * examples' own esbuild settings (scripts/build.mjs) and packs it; returns
 * the `.nailplugin` path. Nothing is written into the repo.
 */
export async function buildExample(name: ExampleName): Promise<string> {
  const example = path.join(repoRoot, "examples", name);
  const staging = path.join(buildOutputDir("e2e"), "plugins", name);
  rmSync(staging, { recursive: true, force: true });
  mkdirSync(staging, { recursive: true });
  cpSync(path.join(example, "manifest.json"), path.join(staging, "manifest.json"));
  cpSync(path.join(example, "locales"), path.join(staging, "locales"), { recursive: true });
  await esbuild.build({
    absWorkingDir: example,
    entryPoints: ["src/index.tsx"],
    outfile: path.join(staging, "dist", "index.js"),
    bundle: true,
    format: "esm",
    platform: "neutral",
    target: "es2023",
    mainFields: ["module", "main"],
    jsx: "automatic",
    jsxImportSource: "@nail/plugin-api",
    charset: "utf8",
    legalComments: "none",
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

/** A plug-in written as files: `manifest` becomes manifest.json, `main` index.js. */
export interface PluginFiles {
  manifest: Record<string, unknown>;
  main: string;
}

/** Writes a plug-in folder (a development folder); returns its path. */
export function writePluginFolder(files: PluginFiles, dir = tempDir("plugin")): string {
  writeFileSync(path.join(dir, "manifest.json"), JSON.stringify(files.manifest, null, 2));
  writeFileSync(path.join(dir, "index.js"), files.main);
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
