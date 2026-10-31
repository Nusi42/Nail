---
name: ssot-odr-dry
description: "Maintainer rule (mandatory): SSOT, ODR and DRY for all existing and new code; reviews treat violations as must-fix"
metadata:
  type: feedback
---

Every fact (type, constant, id, limit, size, string, rule, mapping, schema) has exactly one source of truth and one
definition; other places derive it (generated code, imports, shared tokens, shared helpers). Rust ↔ TS facts are
generated (ts-rs / `npm run gen:api`) or pinned by a test, never hand-mirrored. No copy-pasted logic.

**Why:** the maintainer made it mandatory for source code reviews, for existing and new code.
**How to apply:** the rule is in CLAUDE.md's non-negotiable rules and in `.claude/workflows/nail-wave.js` (every agent's
mandatory rules; every review checks the wave's diff and reports violations as must-fix). The duplicate check
(`npm run check:duplicates`, a blocking step of `npm run check`) stays at 0 findings, like [[zero-warnings-policy]].

Related: [[zero-warnings-policy]], [[next-steps]]
