# Nail

An email and PIM client whose entire UI is defined by two user-editable files
(`theme.kdl`, `layout.kdl`), with a Rust core and a React renderer.

- Design: [`docs/design.md`](docs/design.md)
- Implementation plan: [`docs/plan.md`](docs/plan.md)
- Decisions: [`docs/adr/`](docs/adr/)
- User documentation: [`userDocs/en/`](userDocs/en/)

## Quick start (development)

```sh
npm install                      # npm workspaces: ui, e2e
npm run build                    # builds ui/dist (needed by the Tauri shell)
cargo test --workspace           # Rust unit tests incl. contract checks
npm test                         # Vitest
npm run lint                     # ESLint, TypeScript, Prettier
npm run gen:api                  # regenerate ui/src/api/generated from nail-api
```

Build output must stay outside the OneDrive-synced repository: create a local,
gitignored `.cargo/config.toml` with `[build] target-dir = "<path outside the repo>"`.
