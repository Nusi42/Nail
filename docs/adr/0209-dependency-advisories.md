# 0209 — Dependency advisories: fixes, accepted risks and a release gate

Status: accepted (2026-10-31)

## Context

GitHub's Dependabot reported 34 open alerts on the public repository after
0.8.1: `zip` 8.6 (one critical, three high, one medium), `rustls` 0.23.45
(in `Cargo.lock` and in the copy of `scripts/interop`), `glib` 0.18.5, and
19 alerts of npm development packages. `npm run check` had not reported
any of them:

- `cargo deny` checks advisories only with `--advisories` or `CI=1`
  (the RustSec database comes from the network), and `release:prepare`
  ran plain `npm run check`, so no release ever checked advisories.
- Dependabot reads GitHub's advisory database. The `zip` advisories
  (GHSA-m665-v7pm-7grj, -hjqx-j4rw-g7xv, -9hj8-73fh-9963, -q7v9-x93c-xqv3,
  -8hxf-qr8p-62x4) are not in RustSec: `cargo deny check advisories` on
  the 0.8.1 lock file reports `rustls` but not `zip`.
- Conversely, RustSec had advisories Dependabot did not show: `ammonia`
  4.2.0 (the mail HTML sanitizer: CSS `url()` escaping and case, SVG
  animation tags), `rustls-webpki` 0.103.15 and `webpki-roots` 1.0.9
  (distrusted intermediates), `tokio` 1.53.1 (`AsyncFd` I/O safety),
  `zstd` / `zstd-safe` / `zstd-sys` (through `tantivy`), `ctutils` and
  `crypto-bigint` (timing), and a yanked `yoke-derive`.
- `npm audit` also listed `zod` (development: `eslint-plugin-react-hooks`)
  and, after the update, `re2js` (development: `markdownlint`).

## Decision

### Fixed

- **`zip` 9.2** (newest 9.x, at least the fixed 9.0.5), still without
  default features (`deflate-flate2-zlib-rs` only: no AES, no legacy
  Shrink / Reduce / Implode, no bzip2 / zstd / lzma). `ZipFile::name()`
  and `ZipArchive::file_names()` return `Result` now; the call sites map
  the error (an unreadable name is an unsafe path or a corrupt backup).
- **Nail's own zip reading, independent of the library fix.** Nail never
  extracts with `zip`'s `extract*` APIs and never writes an entry to a
  path taken from the archive: plug-in packages are read into memory and
  kept packed (ADR 0051), backup entries are hashed against the manifest
  and handed to the core by name. Per advisory:
  - *Dual-header inconsistency / zip cloaking (critical):* both readers
    now call `ZipArchive::validate_header_consistency()` after opening —
    every local header must match its central directory entry, no
    overlapping entries, no local header the central directory does not
    list — and refuse `has_duplicate_names()` first. Before this, an exact
    duplicate name was silently dropped by the central directory index
    (`zip` keeps one entry per name), so the case-insensitive duplicate
    check never saw it.
  - *File write outside the extraction dir, unbounded symlink targets:*
    not reachable (no extraction to disk); plug-in packages refuse symlink
    entries (`PackageErrorKind::Link`) and every path through
    `is_safe_path`; backup entry names go through `valid_entry_name`; a
    restored plug-in's id must now be a valid plug-in id (one path segment)
    before it becomes part of a temporary file name.
  - *Legacy decoder DoS, AES HMAC bypass:* the features are not compiled
    in, and plug-in packages refuse every method but stored / deflate and
    every encrypted entry before decompressing.
  - *Zip bombs:* unchanged — packages read with a running budget of
    `MAX_PACKAGE_BYTES`, backup entries with per-kind in-memory limits.
  - Regression tests with crafted archives: exact duplicate names, a local
    header naming another file than the central directory, Shrink / Reduce
    / Implode / AES method ids (`nail-plugins` `package` tests); duplicate
    and cloaked entries in an encrypted backup (`nail-portability` backup
    tests); restored plug-in ids (`nail-core` backup tests). The existing
    tests cover traversal, absolute and device paths, symlinks, other
    methods, encryption and zip bombs.
- **`rustls` 0.23.46** in both lock files (the interop copy is the root
  lock file pruned by cargo, `scripts/interop/README.md`). Nail has no
  custom session store and never formats a session value; the connection
  types' `Debug` (inside the `derive(Debug)` IMAP / POP3 / ManageSieve
  stream enums) prints no fields.
- **Semver-compatible updates** for the RustSec findings: `ammonia` 4.2.4,
  `rustls-webpki` 0.103.16, `webpki-roots` 1.0.11, `tokio` 1.54.1,
  `ctutils` 0.4.7, `zstd` 0.14.3 / `zstd-safe` 8.2 / `zstd-sys` 2.1.2,
  `yoke-derive` 0.8.4.
- **`tantivy` 0.27** (from 0.26.2): it moves `tantivy-sstable` from the
  vulnerable `zstd` 0.13 to 0.14. Same index format (7); Nail's analyzer
  (simple tokenizer, lower case, ASCII folding) does not use the stemmer
  that changed; the code compiles unchanged and an index that does not open
  is rebuilt anyway.
- **npm (development only):** `npm audit fix` within the declared ranges
  (`vite` 8.3.5 with the fixed `launch-editor`, `vitest` / `@vitest/mocker`
  5.0.5, `undici` 8.12.3, `source-map-js` 1.2.2, `zod` 4.6.7);
  `markdownlint-cli2` ^0.24.0 (fixed `markdown-it` 15.0.3 and `smol-toml`
  1.9.1); overrides `re2js` ~2.8.7 under `markdownlint` (it pins 2.8.6) and
  `katex` ^0.18.2 under `micromark-extension-math` (only its syntax is
  used; nothing renders math). None of these packages is in the shipped UI:
  `npm audit --omit=dev` reports nothing and `ui/dist` contains none of
  them. Vitest 5.0.5's jsdom environment has `URL.createObjectURL`, so the
  reader test of a failed attachment download now makes it fail on purpose
  (`ui/src/widgets/m5bMail.test.tsx`) instead of relying on its absence.

### Accepted risks

- **`glib` 0.18.5** (GHSA-wrw7-89jp-8q8g, unsound `VariantStrIter`, fixed
  in 0.20): Linux only, through Tauri 2's `gtk` 0.18 / `webkit2gtk` 2.0 /
  `wry` 0.57. Every Tauri 2 release (2.12.5 the newest) and `wry` 0.57.1
  stay on GTK 3 (`gtk` ^0.18); only the Tauri 3 alphas move to `gtk4`. Not
  compiled on Windows. Nothing in the Linux dependency tree calls
  `Variant::array_iter_str` or `VariantStrIter` (searched in every crate's
  source of `cargo tree --target x86_64-unknown-linux-gnu`), Nail has no
  `glib` code. The maintainer dismisses the alert on GitHub with this
  reason; it goes away with Tauri 3.
- **`crypto-bigint` 0.5.5** (RUSTSEC-2026-0387, secret-dependent branches,
  fixed in 0.7.6): `elliptic-curve` 0.13 (`pgp` 0.20 and 0.21, `cx448`,
  `p256` / `p384` / `p521`, `ecdsa`) is pinned to this generation, like
  `rsa` 0.9 (ADR 0034). A local timing side channel while Nail signs or
  decrypts; no network-facing oracle. Ignored in `deny.toml` with this
  reason until the next RustCrypto generation.
- **`minimatch` < 11.0.1** (GHSA-g3hv-75wg-m249, development): pulled in by
  ESLint, `typescript-eslint`, `editorconfig` (^10) and the React ESLint
  plug-ins (^3); the fix exists only in 11, which none of them accepts. The
  patterns come from the repository's own configuration, never from input.
  The maintainer dismisses the alert with this reason.

### Release gate

`release:prepare` (unless `--skip-checks`) now:

1. refuses while the repository has open Dependabot alerts (`gh api
   repos/<slug>/dependabot/alerts?state=open`, `scripts/lib/release.mjs`
   `dependabotAlertsArgs` / `openAlerts`): GitHub's database is the one
   that knew about `zip`. An accepted risk is dismissed on GitHub with its
   reason, so the gate stays meaningful;
2. runs `npm run check -- --advisories` (`cargo deny` with RustSec)
   instead of plain `npm run check`;
3. runs `npm audit --omit=dev` (what the UI ships).

The everyday `npm run check` and the hooks stay offline: a push must not
depend on GitHub, and a new advisory must not block unrelated work.

## Alternatives

- **Advisories in the pre-push hook:** catches them earlier, but every push
  would need the network and fail on advisories nobody can fix yet.
  Rejected; the release is the point where shipped code leaves.
- **A scheduled job** (GitHub Actions with `cargo deny` and `npm audit`):
  Nail has no hosted CI (docs/dev-setup.md "Verification (no hosted CI)");
  Dependabot already plays this role on GitHub. Its alerts now block the
  release instead of waiting to be noticed.
- **Dropping `zstd` from `tantivy`** (`default-features = false` without
  `columnar-zstd-compression`): would make existing indexes with
  zstd-compressed columns unreadable (they would be rebuilt). Upgrading
  `tantivy` keeps them.
- **Overriding `minimatch` to 11** for the ^10 users: a major version under
  ESLint's feet for a pattern-only DoS in development tools. Rejected.

## Consequences

- The next `release:prepare` fails until the `glib` and `minimatch` alerts
  are dismissed on GitHub (or fixed); the message lists every open alert.
- `release:prepare` needs `gh` signed in (as `release:publish` already did)
  and network for the advisory databases.
- A new RustSec advisory without a fix gets an `ignore` entry in
  `deny.toml` with its reason and ADR, like RUSTSEC-2023-0071 and
  RUSTSEC-2026-0387.
