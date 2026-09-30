# @nail/plugin-api

Types and helpers for writing Nail plug-ins (preview, M2d). The types in
`src/generated/` come from the Rust contract (`crates/nail-api`,
`export_plugin_typescript`) and are regenerated with `npm run gen:api`; a
Rust test fails when they are stale.

Owned by work package WP63 (see `docs/adr/0050-m2d-contract.md`).
