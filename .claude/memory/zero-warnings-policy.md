---
name: zero-warnings-policy
description: Requirement: linters and formatters for every kind of file in the repo, and zero warnings and zero errors, without switching off good lints or compiler warnings
metadata:
  type: feedback
---

Requirement: linters and formatters for the code, 0 warnings and 0 errors, without disabling linter or compiler warnings and errors that are usually good.

**Why:** the codebase should be clean and strict, not quiet because warnings are suppressed.
**How to apply:**

- Every file kind gets a formatter/linter: Rust (rustfmt, clippy incl. curated pedantic), TS (ESLint type-aware strict + Prettier everywhere incl. e2e/packages/scripts), TOML, Markdown, CI YAML, spelling.
- Fix warnings instead of suppressing them.
- An unavoidable exception uses `#[expect(lint, reason = "…")]` (Rust) or an `eslint-disable-next-line rule -- reason` comment, never a blanket allow.
- Never relax a lint level or `-D warnings` to get green.
- `npm run check` runs every formatter and linter (dev-setup 'Linters and formatters'); CI `quality` blocks.

Related: [[ssot-odr-dry]]
