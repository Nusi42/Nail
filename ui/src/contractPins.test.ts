/**
 * Pins of lists the UI shares with the Rust core and the e2e harness (Q2,
 * `scripts/duplicates/allow.json` names this file as `pinnedBy`): the UI
 * keeps its own copy (the theme compiler runs in the browser, the mock core
 * mirrors nail-config, the visual harness runs under plain `node --test`),
 * and this test fails as soon as either side changes alone.
 *
 * - rust-ts-list "active, focus, focus-visible, focus-within, hover":
 *   `PSEUDO_STATES` = `nail_api::config::PSEUDO_CLASS_STATES`
 * - rust-ts-list "font-size, radius, size, space":
 *   `PX_CATEGORIES` = `nail_config::css::PX_TOKEN_CATEGORIES`
 * - const-value `ts DENSITIES = ["comfortable", "compact"]`:
 *   the appearance page's `DENSITIES` = the visual harness's (`e2e/visual/lib/matrix.ts`)
 */
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { DENSITIES } from "./features/appearance/model";
import { PX_CATEGORIES } from "./features/appearance/tokens";
import { PSEUDO_STATES } from "./theme/compile";

/** A repository file (Vitest runs in `ui/`). */
function repoFile(path: string): string {
  return readFileSync(resolve(process.cwd(), "..", path), "utf8");
}

/** The string literals of the array literal that follows `name` in `text`. */
function listAfter(text: string, name: string): string[] {
  const at = text.indexOf(name);
  expect(at, name).toBeGreaterThanOrEqual(0);
  const open = text.indexOf("[", text.indexOf("=", at));
  const close = text.indexOf("]", open);
  return [...text.slice(open, close).matchAll(/"([^"]*)"/g)].map((m) => m[1] ?? "");
}

describe("lists shared with Rust and the e2e harness", () => {
  it("compiles the same states to pseudo-classes as the core", () => {
    const rust = listAfter(
      repoFile("crates/nail-api/src/config/theme.rs"),
      "const PSEUDO_CLASS_STATES",
    );
    expect([...PSEUDO_STATES]).toEqual(rust);
  });

  it("treats the same token categories as pixel lengths as nail-config", () => {
    const rust = listAfter(repoFile("crates/nail-config/src/css.rs"), "const PX_TOKEN_CATEGORIES");
    expect([...PX_CATEGORIES].sort()).toEqual([...rust].sort());
  });

  it("captures the visual matrix in every density the appearance page offers", () => {
    const harness = listAfter(repoFile("e2e/visual/lib/matrix.ts"), "export const DENSITIES");
    expect([...DENSITIES]).toEqual(harness);
  });
});
