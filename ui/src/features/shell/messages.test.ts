// @vitest-environment node
/**
 * The shell's strings: every message id its sources use exists in the
 * English catalogs, `shell.ftl` holds no unused ids, and the German file
 * stays empty (per-key English fallback). The app-wide coverage test does
 * the same for every feature once `messages.ts` is listed there.
 */
import { readFileSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";
import { FluentResource } from "@fluent/bundle";
import { prefixes, notMessages, runtimeIds } from "./messages";

const shellDir = resolve(process.cwd(), "src/features/shell");
const localesDir = resolve(process.cwd(), "../locales");

function ids(file: string): string[] {
  return new FluentResource(readFileSync(file, "utf8")).body.map((entry) => entry.id);
}

function catalog(locale: string): Set<string> {
  const dir = join(localesDir, locale);
  return new Set(
    readdirSync(dir)
      .filter((f) => f.endsWith(".ftl"))
      .flatMap((f) => ids(join(dir, f))),
  );
}

/** Literal ids with one of the shell's prefixes in its non-test sources. */
function literalIds(): string[] {
  const out = new Set<string>();
  for (const file of readdirSync(shellDir)) {
    if (!/\.tsx?$/.test(file) || /\.test\.tsx?$/.test(file)) continue;
    const source = readFileSync(join(shellDir, file), "utf8");
    for (const match of source.matchAll(/["'`]([a-z][a-z0-9-]*[a-z0-9])["'`]/g)) {
      const literal = match[1] ?? "";
      if (prefixes.some((p) => literal.startsWith(p)) && !notMessages.includes(literal)) {
        out.add(literal);
      }
    }
  }
  return [...out];
}

describe("shell strings", () => {
  const used = [...literalIds(), ...runtimeIds()];

  it("finds the ids the shell uses", () => {
    expect(used).toEqual(
      expect.arrayContaining([
        "nav-rail-label",
        "view-action-next",
        "shell-rail-tooltip",
        "shell-view-region",
        "shell-rail-item-unread",
      ]),
    );
  });

  it("has every id in the English catalogs", () => {
    const en = catalog("en");
    expect(used.filter((id) => !en.has(id))).toEqual([]);
  });

  it("keeps shell.ftl to used shell- ids", () => {
    const own = ids(join(localesDir, "en", "shell.ftl"));
    expect(own.length).toBeGreaterThan(0);
    expect(own.filter((id) => !id.startsWith("shell-"))).toEqual([]);
    expect(own.filter((id) => !used.includes(id))).toEqual([]);
  });

  it("leaves the German file empty", () => {
    expect(ids(join(localesDir, "de", "shell.ftl"))).toEqual([]);
  });
});
