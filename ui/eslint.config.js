// ESLint flat config for the Nail renderer.
//
// Besides the usual TypeScript/React checks this enforces two project rules
// (docs/plan.md, "Ground rules"):
//   1. No literal user-visible strings in JSX: text comes from Fluent catalogs.
//   2. No inline styles and no hard-coded class names: components emit
//      `data-part` attributes and all styling comes from the compiled theme.
// `src/lint-rules.test.ts` verifies that these rules fire.
import js from "@eslint/js";
import prettier from "eslint-config-prettier";
import react from "eslint-plugin-react";
import reactHooks from "eslint-plugin-react-hooks";
import globals from "globals";
import tseslint from "typescript-eslint";

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

export default tseslint.config(
  { ignores: ["dist/", "coverage/", "src/api/generated/"] },
  js.configs.recommended,
  ...tseslint.configs.strict,
  {
    files: ["**/*.{ts,tsx}"],
    languageOptions: {
      ecmaVersion: 2023,
      globals: { ...globals.browser },
      parserOptions: { ecmaFeatures: { jsx: true } },
    },
    settings: { react: { version: "19.3" } },
    plugins: { react },
    extends: [reactHooks.configs.flat.recommended],
    rules: {
      ...react.configs.flat.recommended.rules,
      ...react.configs.flat["jsx-runtime"].rules,
      "react/jsx-no-literals": ["error", { noStrings: true, ignoreProps: true }],
      "react/no-danger": "error",
      "react/forbid-dom-props": ["error", { forbid: ["style"] }],
      "no-restricted-syntax": ["error", ...nailRestrictedSyntax],
      "@typescript-eslint/consistent-type-imports": "error",
    },
  },
  {
    // Tests may render literal fixture text; styling rules still apply.
    files: ["**/*.test.{ts,tsx}", "src/test/**"],
    rules: {
      "react/jsx-no-literals": "off",
    },
  },
  {
    files: ["*.config.{js,ts}"],
    languageOptions: { globals: { ...globals.node } },
  },
  prettier,
);
