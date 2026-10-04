// ESLint flat config for the Nail renderer: the shared base
// (../eslint.config.base.js: typescript-eslint strictTypeChecked +
// stylisticTypeChecked, type-aware) plus React, React Hooks, jsx-a11y and the
// two project rules (docs/plan.md, "Ground rules"):
//   1. No literal user-visible strings in JSX: text comes from Fluent catalogs.
//   2. No inline styles and no hard-coded class names: components emit
//      `data-part` attributes and all styling comes from the compiled theme.
// `src/lint-rules.test.ts` verifies that these rules fire.
import { defineConfig } from "eslint/config";
import { base, nailRestrictedSyntax, prettier, react } from "../eslint.config.base.js";

export { nailRestrictedSyntax };

export default defineConfig(
  ...base({ tsconfigRootDir: import.meta.dirname }),
  ...react({
    files: ["**/*.{ts,tsx}"],
    tests: ["**/*.test.{ts,tsx}", "src/test/**"],
    reactVersion: "19.3",
  }),
  prettier,
);
