# ADR 0176: Q2 integration — SSOT / ODR / DRY at 0 findings, the duplicate check blocks

## Context

Wave Q2 (contract ADR 0171) applies the maintainer's rule "single source of truth,
one definition, don't repeat yourself" to the existing code. The contract
recorded 2,076 findings of the duplicate check in ratchet baselines; four
packages removed them in parallel: WP-rust-core (ADR 0172, 584 findings),
WP-rust-engines (ADR 0173, 404), WP-ui (ADR 0174, 783) and
WP-mirrors-tooling (ADR 0175, 305). WP-rust-core and WP-mirrors-tooling ran
out of time before committing; the integrator committed their work trees on
their branches unchanged and finished them on work-1.

## Decisions

- **Merge order** work-3 (engines) → work-2 (core) → work-4 (UI) → work-5 (mirrors).
  Where two packages replaced the same copy, the deeper source wins:
  - generated constants (`ui/src/api/generated/constants.ts`, WP-mirrors-tooling)
    over hand lists the UI had merely pinned (WP-ui's `contractPins.test.ts`
    and its three `pinnedBy` exceptions are gone: `PSEUDO_STATES` and the
    densities are generated, `PX_CATEGORIES` is pinned by
    `scripts/test/rust-ts-pins.test.mjs`);
  - `FOLDER_ROLES` (`ui/src/api/contract.ts`) is now derived from the generated
    `FOLDER_ROLE_VALUES`; the second list `SPECIAL_USE_ROLES` is removed;
  - `nail_api::pim::authority_host` delegates to `nail_api::hosts::authority_host`
    (two packages had each added one); `is_https_or_loopback_url` uses
    `hosts::is_loopback_host`;
  - `nail_util::time` keeps both packages' additions (`parse_iso_date`,
    `weekday_from_monday`, `last_weekday_of_month`, fixture instants);
  - `plugin_http.rs` uses `nail_net::http::{MAX_REDIRECTS, header_pairs}` with the
    core's `pinned_http` helpers and `nail_proxy::read_limited`.
- **Leftovers fixed at the merge:** generated `as const` tuples no longer accept
  any string in `includes` — the call sites use the typed helpers
  (`takesAccounts`) or widen explicitly; unused imports; a `node:assert`
  narrowing in `scripts/test/program.test.mjs`; two `allow.json` entries that
  matched nothing any more; one reasoned exception for two unrelated
  function-local `COUNTER` statics (Message-IDs, MCP tokens).
- **Blocking:** the baselines `scripts/duplicates/{jscpd,css,ssot}-baseline.json`
  are deleted. Without them every finding is new: `npm run check` runs the
  check with `--strict`, the pre-commit and post-edit hooks with `--files`
  (findings touching the staged / edited files), CI through `npm run check`.
  `--update-baseline` stays for a future ratchet wave.
- **Exceptions** (`scripts/duplicates/allow.json`, each with its reason):
  7 constants, 40 Fluent texts (same English, different meanings), 31 magic
  numbers (unrelated values that agree), 2 CSS lengths (hairlines), 8 Rust ↔
  TypeScript lists and 10 constants pinned by tests the bindings cannot replace.
- **ADR numbers:** the packages' `0172-q2-*.md` became 0172 (rust-core),
  0173 (rust-engines), 0174 (UI), 0175 (mirrors / tooling).

## Consequences

- `node scripts/check-duplicates.mjs --strict` reports 0 findings; a new copy,
  hand mirror or repeated text fails the commit.
- Behaviour is unchanged except for the small, listed deliberate changes of
  ADRs 0172–0175 (stricter parsers where copies disagreed, Gmail URLs checked
  against their origin like Graph's, some English diagnostics reworded).
- Follow-ups kept in `docs/plan.md`: the skins' repeated token maps, the
  composer / merge-dialog debounces, `duration_since(UNIX_EPOCH)` copies in
  engine code, nail-dav's content-line layer next to nail-ical's, test-fixture
  instants living in `nail-util`, itip / crypto texts moving to `common.ftl`
  once their harnesses load it, the const-value detector cutting strings at `//`.
