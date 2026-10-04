// ESLint flat config for everything outside ui/ (which has its own config on
// the same base): e2e, packages/*, examples/*, scripts and the root files.
// ESLint uses the config nearest to each file, so `npx eslint .` at the root
// lints the whole repository (ADR 0123).
import { defineConfig } from "eslint/config";
import { base, prettier } from "./eslint.config.base.js";

export default defineConfig(...base({ tsconfigRootDir: import.meta.dirname }), prettier);
