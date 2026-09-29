/**
 * Builds the desktop app for the WebView2 smoke test
 * (`tests/desktop.spec.ts`): `nail-app` with Tauri's `custom-protocol`
 * feature, so it serves the built `ui/dist` instead of the Vite dev server.
 *
 * It uses a target directory of its own next to cargo's
 * (`<build dir>/desktop-target`, see `buildOutputDir` in support/nail.ts):
 * the feature changes the `tauri` crate, and sharing the workspace target
 * directory would rebuild it on every switch between `cargo test` and this.
 * Run `npm run build` first; the UI is embedded at compile time.
 */
import { execFileSync } from "node:child_process";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const base =
  process.env.NAIL_BUILD_DIR ??
  (process.env.LOCALAPPDATA
    ? path.join(process.env.LOCALAPPDATA, "nail-build")
    : path.join(tmpdir(), "nail-build"));
const targetDir = path.join(base, "desktop-target");

execFileSync(
  "cargo",
  ["build", "-p", "nail-app", "--features", "tauri/custom-protocol", "--target-dir", targetDir],
  { cwd: repoRoot, stdio: "inherit" },
);
