/**
 * What every example in `examples/` must do: use the template's build and
 * pack scripts and tsconfig, pass the manifest check, have every text it
 * uses in its English catalog (and the same ids in its other catalogs),
 * build into one self-contained module and pack into a `.nailplugin`.
 */
import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";

import { FluentCatalog } from "@nail/plugin-api/testing";
import { checkManifest, readZip } from "@nail/plugin-api/tools";
import { afterAll, beforeAll, describe, expect, it } from "vitest";

import { TEMPLATE_DIR } from "../scaffold.mjs";
import {
  buildAndPack,
  exampleNames,
  prepareExample,
  readManifest,
  REPO,
  type PreparedExample,
} from "./helpers";

/** Message ids the code passes to `nail.l10n.t` / the local `t` helper. */
function usedIds(dir: string): string[] {
  const ids = new Set<string>();
  const src = path.join(dir, "src");
  for (const file of readdirSync(src)) {
    const code = readFileSync(path.join(src, file), "utf8");
    for (const m of code.matchAll(/\bt\(\s*"([a-z][a-z0-9-]*)"/g)) ids.add(m[1] ?? "");
  }
  return [...ids].sort();
}

describe.each(exampleNames())("the %s example", (name) => {
  let example: PreparedExample;
  beforeAll(() => {
    example = prepareExample(name);
  });
  afterAll(() => {
    example.cleanup();
  });

  it("uses the template's scripts and tsconfig", () => {
    const scripts = (file: string) =>
      (JSON.parse(readFileSync(file, "utf8")) as { scripts: Record<string, string> }).scripts;
    expect(scripts(path.join(example.source, "package.json"))).toEqual(
      scripts(path.join(TEMPLATE_DIR, "package.json")),
    );
    for (const file of ["tsconfig.json", ".gitignore"]) {
      const template = file === ".gitignore" ? "gitignore" : file;
      expect(readFileSync(path.join(example.source, file), "utf8"), file).toBe(
        readFileSync(path.join(TEMPLATE_DIR, template), "utf8"),
      );
    }
  });

  it("has a valid manifest and every text in its catalogs", () => {
    const manifest = readManifest(example.source);
    expect(checkManifest(manifest)).toEqual([]);
    expect(manifest.id).toBe(name);
    const locales = manifest.contributes?.locales ?? {};
    expect(Object.keys(locales)).toContain("en");
    const english = new FluentCatalog(
      readFileSync(path.join(example.source, locales.en ?? ""), "utf8"),
    );
    const manifestIds = (JSON.stringify(manifest).match(/%[a-z-]+%/g) ?? []).map((t) =>
      t.slice(1, -1),
    );
    expect(manifestIds.length).toBeGreaterThan(0);
    for (const id of [...manifestIds, ...usedIds(example.source)]) {
      expect(english.has(id), `${id} in en.ftl`).toBe(true);
    }
    // Other languages translate the same messages (and no others).
    for (const [tag, file] of Object.entries(locales)) {
      if (tag === "en") continue;
      const other = new FluentCatalog(readFileSync(path.join(example.source, file), "utf8"));
      expect(other.ids().sort(), `${tag} catalog`).toEqual(english.ids().sort());
    }
  });

  it("builds into one self-contained module and packs it", () => {
    const code = readFileSync(example.bundle, "utf8");
    expect(code).not.toMatch(/^\s*import\s/m);
    expect(code).not.toMatch(/\brequire\(/);
    expect(code).toMatch(/export\s*\{[^}]*\bas default\b[^}]*\}/);
    const locales = Object.values(readManifest(example.source).contributes?.locales ?? {});
    expect(buildAndPack(example.dir, name)).toEqual([
      "manifest.json",
      "dist/index.js",
      ...locales.sort(),
    ]);
    const zip = readZip(readFileSync(path.join(example.dir, "dist", `${name}.nailplugin`)));
    expect(zip[1]?.data.toString("utf8")).toBe(code);
  });

  it("is listed in examples/README.md", () => {
    const readme = readFileSync(path.join(REPO, "examples", "README.md"), "utf8");
    expect(readme).toContain(`## \`${name}\``);
  });
});

// Review M4a: build output never goes into the repository (OneDrive), so
// the docs never tell anyone to build an example in place.
describe("example build docs", () => {
  it("build a copy outside the repository", () => {
    const readme = readFileSync(path.join(REPO, "examples", "README.md"), "utf8");
    const setup = readFileSync(path.join(REPO, "docs", "dev-setup.md"), "utf8");
    expect(readme).toContain("copy the example out of the repository");
    expect(readme).not.toMatch(/cd examples\/</);
    expect(readme).not.toMatch(/node examples\/<name>\/scripts\/build\.mjs/);
    expect(setup).not.toMatch(/--prefix examples\//);
  });
});
