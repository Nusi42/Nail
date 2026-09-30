#!/usr/bin/env node
/**
 * Bundles src/index.tsx into the manifest's `main` (dist/index.js) with
 * esbuild: one ES module with every dependency inlined and JSX turned into
 * component trees by @nail/plugin-api's runtime. Nail runs the bundle in
 * a sandbox without Node or browser APIs, so the platform is "neutral".
 *
 *   node scripts/build.mjs           build once
 *   node scripts/build.mjs --watch   rebuild on change (with `nail plugin dev .`)
 */
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import * as esbuild from "esbuild";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const manifest = JSON.parse(readFileSync(path.join(root, "manifest.json"), "utf8"));

/** @type {import("esbuild").BuildOptions} */
const options = {
  absWorkingDir: root,
  entryPoints: ["src/index.tsx"],
  outfile: manifest.main,
  bundle: true,
  format: "esm",
  platform: "neutral",
  target: "es2023",
  mainFields: ["module", "main"],
  jsx: "automatic",
  jsxImportSource: "@nail/plugin-api",
  charset: "utf8",
  legalComments: "none",
  logLevel: "info",
};

if (process.argv.includes("--watch")) {
  const context = await esbuild.context(options);
  await context.watch();
} else {
  await esbuild.build(options);
}
