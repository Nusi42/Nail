// @vitest-environment node
/**
 * Project rule (docs/plan.md, "Everything themeable"): components never use
 * inline styles or literal class names. ESLint enforces this too; this test
 * is a second, lint-independent check over the component sources.
 */
import { readFileSync } from "node:fs";
import { relative } from "node:path";
import { SRC_DIR as srcDir, sourceFiles } from "./test/sourceFiles";

const componentSources = (dir: string) =>
  sourceFiles(dir, { include: /\.tsx$/, exclude: /\.test\.tsx$/ });

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
