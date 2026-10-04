// @vitest-environment node
import { fileURLToPath } from "node:url";
import { ESLint } from "eslint";

const uiRoot = fileURLToPath(new URL("..", import.meta.url));

let eslint: ESLint;

beforeAll(async () => {
  // The fixtures are linted as text under file names that do not exist on
  // disk, so the type-aware project service needs them in its default
  // project (typed like src/ through tsconfig.app.json).
  eslint = new ESLint({
    cwd: uiRoot,
    overrideConfig: {
      languageOptions: {
        parserOptions: {
          projectService: {
            allowDefaultProject: ["src/LintFixture.tsx", "src/Warmup.tsx"],
            defaultProject: "tsconfig.app.json",
          },
        },
      },
    },
  });
  // Warm up: loading the TypeScript parser and plugins takes a few seconds.
  await eslint.lintText("export {};\n", { filePath: "src/Warmup.tsx" });
}, 60_000);

async function lint(code: string): Promise<string[]> {
  const [result] = await eslint.lintText(code, { filePath: "src/LintFixture.tsx" });
  return (result?.messages ?? []).map((m) => m.ruleId ?? m.message);
}

describe("project lint rules", () => {
  it("accepts a component that only emits data-part attributes", async () => {
    const code = [
      "export function Ok({ label }: { label: string }) {",
      '  return <div data-part="ok" aria-label={label}>{label}</div>;',
      "}",
      "",
    ].join("\n");
    expect(await lint(code)).toEqual([]);
  });

  it.each([
    [
      "inline style",
      'export const A = () => <div style={{ color: "red" }} />;\n',
      "no-restricted-syntax",
    ],
    [
      "literal className",
      'export const A = () => <div className="x" />;\n',
      "no-restricted-syntax",
    ],
    [
      "className expression",
      'export const A = (p: { on: boolean }) => <div className={p.on ? "a" : "b"} />;\n',
      "no-restricted-syntax",
    ],
    [
      "template className",
      "export const A = (p: { v: string }) => <div className={`a-${p.v}`} />;\n",
      "no-restricted-syntax",
    ],
    ["JSX text", "export const A = () => <div>Hello</div>;\n", "react/jsx-no-literals"],
    ["string child", 'export const A = () => <div>{"Hello"}</div>;\n', "react/jsx-no-literals"],
    [
      "literal aria-label",
      'export const A = () => <button aria-label="Close" />;\n',
      "no-restricted-syntax",
    ],
    ["literal title", 'export const A = () => <span title={"Tip"} />;\n', "no-restricted-syntax"],
    [
      "dangerouslySetInnerHTML",
      "export const A = (p: { h: string }) => <div dangerouslySetInnerHTML={{ __html: p.h }} />;\n",
      "react/no-danger",
    ],
    // Quality baseline (ADR 0123): jsx-a11y, type-aware strict rules and
    // described, rule-specific eslint-disable comments.
    [
      "image without alt text",
      "export const A = (p: { src: string }) => <img src={p.src} />;\n",
      "jsx-a11y/alt-text",
    ],
    [
      "floating promise",
      "export function f(p: () => Promise<void>) {\n  p();\n}\n",
      "@typescript-eslint/no-floating-promises",
    ],
    [
      "eslint-disable without a reason",
      "// eslint-disable-next-line @typescript-eslint/no-floating-promises\nexport function f(p: () => Promise<void>) {\n  p();\n}\n",
      "@eslint-community/eslint-comments/require-description",
    ],
    [
      "blanket eslint-disable",
      "/* eslint-disable */\nexport const a = 1;\n",
      "@eslint-community/eslint-comments/no-unlimited-disable",
    ],
  ])("rejects %s", async (_name, code, rule) => {
    expect(await lint(code)).toContain(rule);
  });
});
