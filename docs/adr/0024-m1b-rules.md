# ADR 0024: Rules engine — rules.kdl, planning engine, rules UI

Status: accepted · 2026-09-29 · M1b WP22

## Context

WP22 (plan section 6) adds filter rules on the ADR 0022 contract
(`nail_api::rules`, store v4 `rule_runs` / `rule_run_actions`). The design
("Filters & rules") asks for a text form in the config language, a visual
editor, dry runs, "create rule from message", and runs that go through
the operation queue and can be undone per run.

## Decisions

### rules.kdl (`nail-config::rules`)

- Syntax: top-level `rule "<name>"` nodes only, with `id=`, `enabled=`,
  shorthand `trigger="arrival|on-send|manual"` and `scope="<spec>"`;
  children `trigger "<kind>" [every=… | days=…]`, `scope "<spec>"
  [account=…]`, one `when [all|any] { … }`, one `then { … }`. Scope specs:
  `account:`, `virtual:` (both `RuleScope::Account`), `folder:`, `role:`.
  The design example parses as written. Condition and action node names
  are listed in `userDocs/en/rules.md` and the schema
  (`schema/rules.schema.json`, `nail_config::rules::rules_schema`).
- `header "X" exists=#false` reads as `not { header "X" }`; `not` with
  several children is `not(all(…))`. The conditions' root is always an
  `all` / `any` group after reading (`normalize_rule`).
- Ids: `id=` or derived from the name (`rule_id_base`, `-2`, `-3` for
  duplicates; explicit ids are reserved first). The writer emits `id=`
  only when it differs from the base, so renamed rules keep their id.
- **All errors at once**: the reader collects one error per broken rule,
  trigger, scope, condition or action (with file, line, column) and keeps
  reading; `parse_rules` returns `Vec<ConfigError>`. This matches WP24's
  "every error of a file" and `RuleParsed.errors`.
- Regular expressions and search queries are validated in `nail-rules`
  (nail-config does not depend on `regex` / `nail-search`).
- **Format-preserving writer** (`RulesDocument`): unchanged rules stay
  byte for byte with their attached comment block; rule "places" in the
  file stay fixed and reordering permutes rules over them (free-standing
  comments stay put); a changed rule is merged node by node against its
  canonical rendering — structurally equal nodes (header, `when`, `then`,
  single conditions / actions) keep their text and leading comments,
  similar nodes (same name + first argument) recurse. It is text splicing
  with a small lexical scanner (spans of the `kdl` crate include
  neighbouring whitespace/comments and were not usable). Every edit is
  re-read and compared; on mismatch the canonical form of the whole file
  is written (as for `accounts.kdl`). A randomized test (200 edits) checks
  the text always means the rules and comments survive.

### Engine (`nail-rules`)

- The engine **plans**, the core **executes**: `RuleSet::process(cause,
  message, env)` → `MessageOutcome` of `PlannedAction {action, effect}`.
  `Effect::Queue {op, undo}` carries `nail_store::Operation`s: the forward
  op and its inverse computed from the message state before the action
  (flags/tags restored, moves and trash-deletes moved back). Permanent
  deletes, deletes in Trash, copies and sends have no undo; a run whose
  actions all have inverses is undoable (matches `Store::complete_run`).
  Undo = queue the `undo` ops in reverse order.
- Data access through traits: `MessageView` (headers, addresses, subject,
  body, attachments, size, date, flags, tags, account, folder) and
  `Mailbox` (folders, real and virtual accounts); `Env` adds the clock and
  UTC offset. `MessageData` / `StaticMailbox` are in-memory versions.
- Semantics: rules run in file order; a later rule sees the state earlier
  rules left (folder, flags, tags); `stop` ends processing (actions after
  it in the same rule do not run); a delete ends processing; the last
  `notify` wins. Disabled rules never run automatically but can be run by
  hand. `older-than` implies an age requirement for scheduled runs (and
  dry / manual runs of rules without arrival/on-send triggers);
  `next_run_at` / `due` compute schedules (`older-than` checked hourly).
- Query condition: `nail_search::parse_query` + a local evaluator over
  the message (no index); `*` values are wildcards (`from:*@substack.com`).
- `Authentication-Results`: only the topmost header is trusted (lower ones
  can be forged by the sender).
- Replies from rules are never sent to automatic, bulk or list mail or to
  bounce / no-reply senders (RFC 3834) — `SkipReason::AutomaticMail`.
- Regexes are built with a 1 MiB size limit.
- Dry run: `test_rule` → `TestMatch {actions (up to stop), planned, hits}`;
  `ConditionHit {path, matched, value}` explains every evaluated
  condition (groups evaluate all children when explaining).
- `validate(rule)` lists every problem (`RuleIssue` with location).

### UI (`ui/src/features/rules`)

- Widget `rule-manager` (list + editor), dialog `rules` (the same, used
  when the layout has no `window "rules"`), actions `rules.open` and
  `rules.create-from-message` (pre-fills from the selected message's
  sender and `[list]` subject prefix).
- Editor state is component-local; which rule is edited lives in a small
  Zustand store per `NailClient`. The text view uses `RuleToText` /
  `RuleFromText`; the edited rule keeps its id whatever the text says.
- The dry-run explanation is read from an optional `explanation` field on
  `RuleMatch` (see follow-ups); the mock already sends it.
- The mock core implements every rules request (`mock.ts`, `mockText.ts`
  mirrors the canonical syntax, `mockEval.ts` evaluates over mock data).

## Consequences / integration needs

- `nail_api::rules::RuleMatch` should gain `explanation: Vec<ConditionHit>`
  (path, matched, value) — without it the preview shows no "why".
- The core (`nail-core/src/rules.rs`) owns `RulesDocument`, builds
  `MessageView`s from the store, executes plans (enqueue ops, record
  `RuleRunAction {op_id, action, undo}`), sends forwards / replies through
  the composer, runs arrival rules from `rule_inbox` and schedules from
  `rule_state`.
- Sieve / server-side execution is M2.
