# Nail

An email and PIM client whose entire UI is defined by two user-editable files
(`theme.kdl`, `layout.kdl`), with a Rust core and a React renderer.

- Design: [`docs/design.md`](docs/design.md)
- Implementation plan: [`docs/plan.md`](docs/plan.md)
- Decisions: [`docs/adr/`](docs/adr/)
- Developer setup: [`docs/dev-setup.md`](docs/dev-setup.md)
- User documentation: [`userDocs/en/`](userDocs/en/)

## Quick start (development)

```sh
npm install                                        # npm workspaces: ui, e2e
npm run build                                      # builds ui/dist
cargo run -p nail-cli -- serve --seed demo --open  # the web UI with demo mail
cargo test --workspace                             # Rust tests
npm test                                           # Vitest
npm run e2e                                        # Playwright
```

See [`docs/dev-setup.md`](docs/dev-setup.md) for prerequisites, all
commands and the desktop app. Build output must stay outside the
OneDrive-synced repository: create a local, gitignored `.cargo/config.toml`
with `[build] target-dir = "<path outside the repo>"`.
