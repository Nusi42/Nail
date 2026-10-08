/**
 * Builds the desktop app for the WebView2 smoke tests
 * (`tests/desktop.spec.ts`, `tests/desktop-m4b.spec.ts`): `nail-app` with
 * Tauri's `custom-protocol` feature, so it serves the built `ui/dist`
 * instead of the Vite dev server; plus `nail_mapi.dll` with
 * `nail-mapi-harness` and `nail-fake-installer` (M4b).
 *
 * It uses a target directory of its own next to cargo's
 * (`<build dir>/desktop-target`, see `buildOutputDir` in support/nail.ts):
 * the feature changes the `tauri` crate, and sharing the workspace target
 * directory would rebuild it on every switch between `cargo test` and this.
 * Run `npm run build` first; the UI is embedded at compile time.
 *
 * The app and the DLL are built with their test-only `test-overrides`
 * features (M4b review):
 * the app then honours NAIL_REGISTRY_ROOT, NAIL_UPDATE_ENDPOINT /
 * NAIL_UPDATE_PUBKEY and NAIL_MAPI_PIPE, the DLL NAIL_MAPI_PIPE /
 * NAIL_MAPI_APP. Installer builds (scripts/build-installer.mjs) never do.
 */
import { execFileSync } from "node:child_process";
import path from "node:path";

import { buildBase } from "../../scripts/lib/build.mjs";
import { repoRoot } from "../../scripts/lib/program.mjs";
import { assertWorkFolder } from "../../scripts/lib/work-folder.mjs";

assertWorkFolder("npm run build:desktop", repoRoot);
const base = buildBase(process.env, repoRoot);
const targetDir = path.join(base, "desktop-target");

execFileSync(
  "cargo",
  [
    "build",
    "-p",
    "nail-app",
    "--features",
    "tauri/custom-protocol,test-overrides",
    "--target-dir",
    targetDir,
  ],
  { cwd: repoRoot, stdio: "inherit" },
);
// M4b (WP115): the Simple MAPI DLL + its harness (a program calling
// MAPISendMailW) with the test-only overrides, then the fake installer the
// updater scenario serves. Two calls: cargo ignores `--features
// <package>/<feature>` when several packages are selected (it built the DLL
// without the overrides), and a plain feature name must exist in every
// selected package.
execFileSync(
  "cargo",
  [
    "build",
    "-p",
    "nail-mapi",
    "--lib",
    "--bin",
    "nail-mapi-harness",
    "--features",
    "test-overrides",
    "--target-dir",
    targetDir,
  ],
  { cwd: repoRoot, stdio: "inherit" },
);
execFileSync(
  "cargo",
  ["build", "-p", "nail-testkit", "--bin", "nail-fake-installer", "--target-dir", targetDir],
  { cwd: repoRoot, stdio: "inherit" },
);
