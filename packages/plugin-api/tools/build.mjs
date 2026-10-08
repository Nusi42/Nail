/**
 * Bundles a plug-in's `src/index.tsx` into the manifest's `main` (e.g.
 * `dist/index.js`) with esbuild: one ES module with every dependency inlined
 * and JSX turned into component trees by `@nail/plugin-api`'s runtime. Nail
 * runs the bundle in a sandbox without Node or browser APIs, so the platform
 * is "neutral". One definition of the build for every plug-in (the
 * scaffolder's template, the examples, the end-to-end tests): `nail-plugin
 * build [dir] [--watch]`.
 */
import { readFileSync } from "node:fs";
import path from "node:path";

/**
 * The esbuild options of the plug-in in `dir`.
 *
 * @param {string} dir the plug-in folder (with `manifest.json` and `src/index.tsx`)
 * @param {{ outfile?: string }} [options] where to write the bundle (default: the manifest's `main`)
 * @returns {import("esbuild").BuildOptions}
 */
export function pluginBuildOptions(dir, options = {}) {
  const root = path.resolve(dir);
  const manifest = /** @type {unknown} */ (
    JSON.parse(readFileSync(path.join(root, "manifest.json"), "utf8"))
  );
  const main =
    typeof manifest === "object" && manifest !== null && "main" in manifest
      ? manifest.main
      : undefined;
  if (options.outfile === undefined && typeof main !== "string") {
    throw new Error('manifest.json needs "main" (the bundle to build, e.g. dist/index.js)');
  }
  return {
    absWorkingDir: root,
    entryPoints: ["src/index.tsx"],
    outfile: options.outfile ?? String(main),
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
}

/**
 * Builds the plug-in in `dir` once, or rebuilds it on every change with
 * `watch` (for `nail plugin dev .`). esbuild is a peer dependency.
 *
 * @param {{ dir: string, watch?: boolean, outfile?: string }} options
 */
export async function buildPlugin({ dir, watch = false, outfile }) {
  const esbuild = await import("esbuild");
  const options = pluginBuildOptions(dir, outfile === undefined ? {} : { outfile });
  if (watch) {
    const context = await esbuild.context(options);
    await context.watch();
  } else {
    await esbuild.build(options);
  }
}
