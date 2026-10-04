// @vitest-environment node
// Real `vite build` runs on tiny projects in a temporary directory (never
// inside the repository): a warning fails the build, a clean build passes.
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { build, type InlineConfig } from "vite";
import { afterEach, describe, expect, it } from "vitest";
import { strictBuild } from "./strictBuild";

const dirs: string[] = [];

afterEach(() => {
  for (const dir of dirs.splice(0)) {
    rmSync(dir, { recursive: true, force: true });
  }
});

/** A project with `index.html` loading `main.js` with this code. */
function project(main: string): string {
  const dir = mkdtempSync(path.join(tmpdir(), "nail-strict-build-"));
  dirs.push(dir);
  writeFileSync(
    path.join(dir, "index.html"),
    '<!doctype html><html><body><script type="module" src="./main.js"></script></body></html>',
  );
  writeFileSync(path.join(dir, "main.js"), main);
  return dir;
}

function config(root: string, extra: InlineConfig["build"] = {}) {
  const strict = strictBuild("silent");
  const inline: InlineConfig = {
    root,
    configFile: false,
    logLevel: "silent",
    customLogger: strict.logger,
    plugins: [strict.plugin],
    build: { outDir: path.join(root, "dist"), emptyOutDir: true, ...extra },
  };
  return { strict, inline };
}

describe("strictBuild", () => {
  it("passes a build without warnings", async () => {
    const root = project('document.body.dataset.ready = "1";\n');
    const { strict, inline } = config(root);
    await expect(build(inline)).resolves.toBeDefined();
    expect(strict.warnings).toEqual([]);
  });

  it("fails on Vite's chunk-size warning", async () => {
    // About 30 kB of code against a 1 kB limit.
    const big = Array.from({ length: 1500 }, (_, i) => `"entry-${String(i)}"`).join(",");
    const root = project(`document.body.dataset.list = [${big}].join(" ");\n`);
    const { strict, inline } = config(root, { chunkSizeWarningLimit: 1 });
    await expect(build(inline)).rejects.toThrow(/zero warnings/);
    expect(strict.warnings.join("\n")).toMatch(/larger than 1 kB/);
  });

  it("fails on a Rolldown warning", async () => {
    const root = project('document.body.dataset.value = String(eval("1 + 1"));\n');
    const { strict, inline } = config(root);
    await expect(build(inline)).rejects.toThrow(/zero warnings/);
    expect(strict.warnings.join("\n")).toMatch(/eval/i);
  });
});
