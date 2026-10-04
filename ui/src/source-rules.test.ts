// @vitest-environment node
/**
 * Project rule (docs/plan.md, "Everything themeable"): components never use
 * inline styles or literal class names. ESLint enforces this too; this test
 * is a second, lint-independent check over the component sources.
 */
import { readFileSync, readdirSync } from "node:fs";
import { join, relative, resolve } from "node:path";

const srcDir = resolve(process.cwd(), "src");

function componentSources(dir: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name !== "generated") out.push(...componentSources(path));
    } else if (entry.name.endsWith(".tsx") && !entry.name.endsWith(".test.tsx")) {
      out.push(path);
    }
  }
  return out;
}

describe("component sources", () => {
  const files = componentSources(srcDir);

  it("finds the components", () => {
    expect(files.length).toBeGreaterThan(10);
  });

  it.each(files.map((f) => [relative(srcDir, f), f]))(
    "%s has no style= or literal className",
    (_name, file) => {
      const source = readFileSync(file, "utf8");
      expect(source).not.toMatch(/\bstyle\s*=\s*\{/);
      expect(source).not.toMatch(/\bstyle\s*=\s*["']/);
      expect(source).not.toMatch(/\bclassName\s*=\s*["'`{]/);
      expect(source).not.toMatch(/dangerouslySetInnerHTML/);
    },
  );
});
