// Shared ESLint flat config for every TypeScript / JavaScript file in Nail
// (quality baseline Q1, ADR 0123). Used by the root `eslint.config.js`
// (e2e, packages, examples, scripts, root files) and by `ui/eslint.config.js`.
//
// - typescript-eslint `strictTypeChecked` + `stylisticTypeChecked`, type-aware
//   through the project service: every file belongs to the nearest
//   tsconfig.json that includes it (TypeScript walks up to the root
//   `tsconfig.json`, which collects the loose Node scripts).
// - Exceptions are `// eslint-disable-next-line rule -- reason` only: unused
//   directives are errors, a directive needs a description and must name its
//   rules (eslint-comments).
// - `react()` adds React, React Hooks, jsx-a11y and Nail's UI rules (no
//   literal user-visible strings, no inline styles or class names).
// - Prettier owns formatting (eslint-config-prettier comes last).
import comments from "@eslint-community/eslint-plugin-eslint-comments/configs";
import js from "@eslint/js";
import prettier from "eslint-config-prettier";
import jsxA11y from "eslint-plugin-jsx-a11y";
import reactPlugin from "eslint-plugin-react";
import reactHooks from "eslint-plugin-react-hooks";
import globals from "globals";
import tseslint from "typescript-eslint";

/** Files that ESLint never reads: build output, dependencies, generated code. */
export const ignores = [
  "**/node_modules/",
  "**/dist/",
  "**/coverage/",
  "**/playwright-report/",
  "**/test-results/",
  "**/target/",
  ".nail-build/",
  "app/src-tauri/gen/",
  // Patterns are relative to the config file that uses this list (the root
  // or ui/), hence `**/`.
  "**/src/api/generated/",
  "**/plugin-api/src/generated/",
  // Test fixtures that only look like JavaScript (a Thunderbird prefs.js).
  "crates/*/tests/fixtures/",
  // Workflow-harness scripts: run by the Claude Code workflow runtime with
  // its own globals (`args`, `agent`, ...), not by Node or a browser.
  ".claude/",
];

/** Attributes whose values are read by users or assistive technology. */
const USER_VISIBLE_ATTRIBUTES =
  "aria-label|aria-description|aria-placeholder|aria-roledescription|aria-valuetext|title|alt|placeholder|label";

/** `no-restricted-syntax` selectors enforcing theme-only styling and i18n. */
export const nailRestrictedSyntax = [
  {
    selector: "JSXAttribute[name.name='style']",
    message: "Inline styles are forbidden. Emit a data-part attribute and style it in the theme.",
  },
  {
    selector: "JSXAttribute[name.name='className'] Literal",
    message:
      "Hard-coded class names are forbidden. Emit a data-part attribute and style it in the theme.",
  },
  {
    selector: "JSXAttribute[name.name='className'] TemplateLiteral",
    message:
      "Hard-coded class names are forbidden. Emit a data-part attribute and style it in the theme.",
  },
  {
    selector: `JSXAttribute[name.name=/^(${USER_VISIBLE_ATTRIBUTES})$/] > Literal`,
    message: "User-visible attribute text must come from the Fluent catalogs (locales/en/*.ftl).",
  },
  {
    selector: `JSXAttribute[name.name=/^(${USER_VISIBLE_ATTRIBUTES})$/] > JSXExpressionContainer > :matches(Literal, TemplateLiteral)`,
    message: "User-visible attribute text must come from the Fluent catalogs (locales/en/*.ftl).",
  },
];

/**
 * The type-aware base for every TS / JS file below `tsconfigRootDir`.
 *
 * @param {{ tsconfigRootDir: string }} options
 * @returns {import("eslint").Linter.Config[]}
 */
export function base({ tsconfigRootDir }) {
  return [
    { ignores },
    {
      linterOptions: {
        reportUnusedDisableDirectives: "error",
        reportUnusedInlineConfigs: "error",
      },
    },
    js.configs.recommended,
    ...tseslint.configs.strictTypeChecked,
    ...tseslint.configs.stylisticTypeChecked,
    comments.recommended,
    {
      languageOptions: {
        ecmaVersion: 2023,
        sourceType: "module",
        parserOptions: { projectService: true, tsconfigRootDir },
      },
      rules: {
        "@typescript-eslint/consistent-type-imports": "error",
        // node:test's test() / describe() return promises the runner awaits
        // itself (the option typescript-eslint documents for this case).
        "@typescript-eslint/no-floating-promises": [
          "error",
          {
            allowForKnownSafeCalls: [
              { from: "package", package: "node:test", name: ["test", "it", "describe", "suite"] },
            ],
          },
        ],
        // Every exception names its rules and says why (`-- reason`).
        "@eslint-community/eslint-comments/require-description": [
          "error",
          { ignore: ["eslint-enable"] },
        ],
        "@eslint-community/eslint-comments/no-unlimited-disable": "error",
        "@eslint-community/eslint-comments/disable-enable-pair": [
          "error",
          { allowWholeFile: true },
        ],
      },
    },
    {
      // Node scripts, tool configs and tests that run in Node.
      files: ["**/*.{js,mjs,cjs}", "**/*.config.{js,ts,mjs}"],
      languageOptions: { globals: { ...globals.node } },
    },
    {
      // The plug-in host prelude: a classic script evaluated by QuickJS
      // (nail-plugins), no Node or browser globals.
      files: ["crates/nail-plugins/src/prelude.js"],
      languageOptions: { sourceType: "script", globals: { ...globals.es2021 } },
    },
  ];
}

/**
 * React, React Hooks, jsx-a11y and Nail's UI rules for the renderer (`ui/`).
 *
 * @param {{ files: string[], tests: string[], reactVersion: string }} options
 * @returns {import("eslint").Linter.Config[]}
 */
export function react({ files, tests, reactVersion }) {
  return [
    { ...reactHooks.configs.flat.recommended, files },
    { ...jsxA11y.flatConfigs.recommended, files },
    {
      files,
      languageOptions: {
        globals: { ...globals.browser },
        parserOptions: { ecmaFeatures: { jsx: true } },
      },
      settings: { react: { version: reactVersion } },
      plugins: { react: reactPlugin },
      rules: {
        ...reactPlugin.configs.flat.recommended?.rules,
        ...reactPlugin.configs.flat["jsx-runtime"]?.rules,
        "react/jsx-no-literals": ["error", { noStrings: true, ignoreProps: true }],
        "react/no-danger": "error",
        "react/forbid-dom-props": ["error", { forbid: ["style"] }],
        "no-restricted-syntax": ["error", ...nailRestrictedSyntax],
      },
    },
    {
      // Tests may render literal fixture text; styling rules still apply.
      files: tests,
      rules: { "react/jsx-no-literals": "off" },
    },
  ];
}

/** Turns off the formatting rules Prettier owns; must come last. */
export { prettier };
