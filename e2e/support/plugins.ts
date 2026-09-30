/**
 * Plug-in packages for the M2d scenarios (ADR 0055): the "Follow up"
 * example built from `examples/follow-up` (esbuild + `@nail/plugin-api`'s
 * packer, output outside the repository), and small hand-written plug-ins
 * packed or laid out as development folders.
 */
import { cpSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import { packPlugin } from "@nail/plugin-api/tools";
import * as esbuild from "esbuild";
import { buildOutputDir, repoRoot, tempDir } from "./nail";

/** Where the global setup leaves the built example (`NAIL_E2E_FOLLOW_UP`). */
export const FOLLOW_UP_ENV = "NAIL_E2E_FOLLOW_UP";

/**
 * Builds `examples/follow-up` into `<build>/e2e/plugins/follow-up` and packs
 * it; returns the `.nailplugin` path. Nothing is written into the repo.
 */
export async function buildFollowUp(): Promise<string> {
  const example = path.join(repoRoot, "examples", "follow-up");
  const staging = path.join(buildOutputDir("e2e"), "plugins", "follow-up");
  rmSync(staging, { recursive: true, force: true });
  mkdirSync(staging, { recursive: true });
  cpSync(path.join(example, "manifest.json"), path.join(staging, "manifest.json"));
  cpSync(path.join(example, "locales"), path.join(staging, "locales"), { recursive: true });
  // The example's own build settings (scripts/build.mjs), written elsewhere.
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
  const out = path.join(staging, "..", "follow-up.nailplugin");
  return packPlugin({ dir: staging, out }).file;
}

/** The example's package (built by the global setup). */
export function followUpPackage(): string {
  const file = process.env[FOLLOW_UP_ENV];
  if (!file) throw new Error(`${FOLLOW_UP_ENV} is not set; the global setup did not run`);
  return file;
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
