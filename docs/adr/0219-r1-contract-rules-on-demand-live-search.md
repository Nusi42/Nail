# 0219 — R1 contract: running rules on demand, live search

Status: accepted (2026-11-20)

## Context

Maintainer feedback of 2026-11-20:

- A rule can only be run by hand on its default messages (its folders, else
  the Inboxes) through Run now in the rules editor. The maintainer wants to
  run a rule on a message selection or a folder: from the message list, from
  the folder tree, and from the rules window itself ("run this / the selected
  rules on the selected folder or mails", so a rule just written can be
  applied at once). Agents cannot run rules at all. The core's `RunRule`
  already takes `message_ids` and `folder_id`, but the UI always sent `[]` /
  `null`.
- The search field's parse error breaks the layout: its explanation is a block
  under the field that pushes the content down.
- Search should run while typing (the maintainer forgets to press Enter).
- In the rules window, switching from Editor to Text and back shows a red error
  below the editor.
- The dry run's result list looks misplaced in the rules window (the design
  review of the rules window runs separately; its redesign is a later wave).

## Decision

### Running several rules: `RunRules`

- New request `RunRules { rule_ids, message_ids, folder_id }`, answered with
  `RulesRan { runs }`. The target is the same as `RunRule`'s: the named
  messages, else the folder, else each rule's own messages.
- **File order, one run per rule.** The rules run one after another in the
  order of `rules.kdl` (not of `rule_ids`; duplicates count once), each as its
  own `RuleRun` with cause `manual`, recorded and announced
  (`RuleRunFinished`) like a single Run now. Each run is undone on its own
  (`UndoRuleRun`); the UI undoes "everything this click did" by undoing each
  run of the answer. One combined run was rejected: the run history and its
  undo are per rule everywhere else (arrival, schedule, Run now), and a
  combined run would need a new store shape for no user-visible gain.
- **`stop` ends only its own rule.** Running rules by hand is "Run now" on each
  of them; it is not the arrival pipeline. A later rule sees the message as the
  earlier rules left it (the operation queue applies locally first).
- **Checked before anything runs.** An empty list is `invalid_request`; an
  unknown rule is `not_found`, a broken one reports its issues (as `RunRule`
  did); in either case no rule runs. An error while running (a store failure)
  is returned; runs that finished before it stay recorded.
- `RunRule` stays (one rule; the core and the mock answer it through the same
  code). Remote sessions: `RunRules` needs the `rules` scope and, like
  `RunRule`, a full grant without a target, else the named messages / folder
  must be inside the grant.

### MCP: `run_rules`

- A write tool `run_rules { rules, message_ids?, folder_id?, idempotency_key? }`:
  rules by id or by exact name as `nail://rules` shows them (a name two rules
  share is refused: name it by its id), 1 to `MAX_RUN_RULES` (100), messages
  up to `MAX_BATCH`. It needs the `rules` and `organize` scopes and a grant for
  all accounts (rules see every account, as `create_rule` / `test_rule` do).
- **Confirmation policy:** the kind whose policy decides is the rules'
  strongest action (`nail_api::mcp::tools::run_rules_kind`): a permanent
  delete is `permanent-delete`, forward / redirect / reply is `send`, a delete
  is `trash`, everything else `organize`. Every kind of action the rules take
  also needs its scopes (`rule_action_kind(..).mcp_scopes()`): a forwarding
  rule needs `send`, a permanently deleting one `permanent-delete`. So running
  rules never does more than the agent may do directly, and asks exactly when
  the same change made directly would (destructive kinds always ask; a decline
  of the elicitation leaves the decision to Nail's confirmation, ADR 0217).
  The confirmation lists the rules, then the messages or the folder; summary
  `confirmation-summary-run-rules`; the counted `sends` are the send actions
  times the named messages (unknown, so 0, for a folder or the rules' own
  messages).
- The result lists the operations queued and, as `created`, the run ids.

### UI contract

- `ui/src/features/rules/api.ts`: `RuleRunTarget { messageIds, folderId }`,
  `RULE_SCOPE_TARGET`, `runRule(client, id, target)`, `runRules(client, ids,
  target)`; the mock core answers `RunRules` (`mock.ts`, file order, one run
  each).
- Live search: `LIVE_SEARCH_PAUSE_MS` (400 ms) and `useDebouncedAction(value,
  ms, action, enabled)` in `ui/src/state/debounce.ts` are the one source of the
  pause and of "act after the pause, or at once on Enter (`flush`)". The pause
  is longer than `TYPING_PAUSE_MS` (suggestions) and the search field's
  `CHECK_DELAY_MS` (250 ms), so a query is checked before it is searched.
- Slots: `ui/src/features/search/` (index + `messages.ts`, prefix
  `search-live-`) with `locales/{en,de}/search.ftl` for the live-search
  package; the rules package keeps `ui/src/features/rules/` and `rules.ftl`.

## Consequences

- The packages of wave R1 (`docs/plan.md`) build on this: WR1 the run-target
  control in the rules window, the "Run rule" submenu in the message-list and
  folder-tree menus, and the Editor / Text switch; WR2 the live search and the
  compact error indicator.
- Agents can apply rules to mail; what they can do through a rule is bounded
  by their grant and asked about as the strongest action would be.
- A rule that moves a message before the next rule runs leaves the next rule
  the moved message; a user who wants the arrival semantics (`stop` across
  rules) still gets them from arrival rules.
