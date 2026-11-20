# 0222 — R2 contract: rules dialog redesign, shared pickers, values from the mail

Status: accepted (2026-11-20)

## Context

After wave R1 the maintainer reviewed the rules window (design review of
2026-11-20, must / should / could items) and asked for all of it: a fixed
footer with one problem badge and Test / Run / Save, a "Run on" target in the
footer, a lower panel with Preview and Recent runs (with time and target),
a compact WHEN / WHERE / IF / THEN form, delete and close confirmations, a
responsive window, a segmented Editor / Text switch, an empty state with
starter rules, a list filter and drag reordering, syntax highlighting in the
text view, and one look in Settings and in the window. Two more requests:

- **Values from the mail:** when the rules window is opened from a message
  (Create rule from message), condition fields offer that message's values
  (sender address / name / domain, recipients, subject, mailing list,
  reply-to, account, folder, header names).
- **Pickers everywhere:** "having no UI control to select things like a
  folder is old style". Every field that names a folder, account, tag,
  address / contact, mailing list or header name gets a browse / search /
  select picker (folders as a searchable tree); free text stays allowed. They
  are used wherever such fields exist: the rules editor, the virtual folder
  editor, the search field's helpers, Settings.

R1 left: "Run rule" as a real hover submenu (a dynamic menu source), rule
names starting with `@` read as Fluent ids in that menu, the search hints
pushing content down, a folder change keeping the typed search text without
running it, and the mock answering `RunRules` with `running` runs where the
core answers finished ones.

The design review also found core problems: an unsaved rule could not
round-trip Editor → Text → Editor (an empty name or an empty value is a parse
error, so the draft was lost), an English "the rule name" inside a Fluent
message, runs without a target, and runs of a deleted rule showing up on a
new rule with the same name (rule ids derive from names).

## Decision

### Core

- **No `id=""`:** `render_rule` already omits an empty id (R1); kept and
  tested.
- **Drafts:** `RuleFromText { text, draft }`. With `draft: true` the core
  reads the text with `nail_config::rules::parse_rule_draft`: an empty rule
  name, an empty comparison, tag, address or other empty value is reported in
  `errors` (with its position) but the rule comes back, so the editor keeps an
  unfinished rule across Editor / Text switches. KDL syntax errors, unknown
  nodes and invalid values still return no rule. The rule checks
  (`nail_rules::validate`) are added as for a strict read, without the
  duplicate empty-name issue (`nail_rules::NAME_EMPTY_ISSUE`). `draft: false`
  (default) is the old strict read.
- **No English in Fluent arguments:** the empty rule name is the existing
  `config-rule-needs-name-value` ("a rule needs a name"); the other `what`
  arguments are KDL key names in backticks (code, not prose).
- **Run targets:** `RuleRun.target: RuleRunTarget` (`scope` | `messages {
  count }` | `folder { folder_id }`), stored in `rule_runs.target` (schema
  v21, `NULL` = `scope`, also for older runs). `RunRule(s)` records the
  request's target; runs started by new mail, sending or the schedule record
  `scope` (their cause says which messages). The time stamps were already
  there (`started_at`, `finished_at`).
- **Runs go with their rule:** `DeleteRule` deletes the rule's runs and their
  actions (`Store::delete_rule_runs`). Rejected: marking them "earlier rule
  with this name", which needs a rule generation in every run for little
  gain; the undo of a deleted rule's run is given up (the user deleted the
  rule). Rules removed by editing `rules.kdl` by hand keep their history as
  before.
- **Message facts:** `GetMessageFacts { message_id }` → `MessageFacts {
  facts }` (`nail_api::rules::MessageFacts`: account, folder, subject, from,
  sender, to, cc, reply-to, the `List-Id` without its phrase, every header
  name once). It reads the message as rules see it (`message_view`: the raw
  headers when stored, else the summary). Remote: the `read` scope and the
  message inside the grant (`check_readable`).
- **Picker values:** `SuggestValues { kind, prefix, limit }` →
  `ValueSuggestions { values }` (`SuggestKind`: `tag` | `mailing-list` |
  `header-name`; `ValueSuggestion { value, label, count }`, most used first,
  at most 100). Tags come from the store (`suggest_tags`); mailing lists
  (with the list's name as `label`) and header names from the newest 200
  stored raw messages (`VALUE_SCAN`). Folders, accounts and addresses keep
  their requests (`ListFolders`, `ListAccounts`, `SuggestAddresses`).
  Remote: the `read` scope; a narrowed grant gets an empty list (the values
  span every account, like collected addresses). Core module
  `crates/nail-core/src/values.rs`.

### UI contract

- **Shared pickers** (`ui/src/widgets/pickers/`): one generic `Picker`
  component for every kind (`PICKER_KINDS`: folder, account, tag, address,
  mailing-list, header-name) instead of one component per kind: the field,
  the list, the keyboard model and the theme parts are the same; only the
  options differ, and they come from a source per kind (`usePickerOptions`,
  `suggestValues`, `suggestKindOf`). Props: `kind`, `value`, `onChange(value,
  option)`, `label`, `accountId` and `folderValue` (`path` for rules, `id`
  elsewhere) for folders, `id`, `placeholder`, `disabled`, `invalid`,
  `describedBy`, `part`. Parts `picker` (`data-kind`), `picker.input`, and
  for the package `PICKER_PARTS` (`picker.browse`, `picker.popup`,
  `picker.search`, `picker.list`, `picker.option` …). Texts
  `locales/{en,de}/pickers.ftl`, prefix `pickers-`. The contract's `Picker`
  is the plain text field; `usePickerOptions` returns nothing yet. The older
  `widgets/FolderPicker.tsx` (the Move window) keeps its name and parts.
- **Values from this mail** (`ui/src/features/rules/fromMessage/`):
  `getMessageFacts` / `useMessageFacts` (working), `valuesForCondition(facts,
  condition)` and the suggestion control `ValuesFromMessage { condition,
  onPick }` (stubs) that the rules editor places beside a condition's value.
  The window learns its message from the rules store's `origin: { messageId,
  folderId } | null` (`setOrigin`): `rules.create-from-message` sets it,
  `rules.open` clears it. Texts `locales/{en,de}/rules-from-message.ftl`
  (prefix `rules-from-message-`, inside the rules prefix).
- **Dynamic menu sources** (`ui/src/features/menus/sources.ts`): `MenuSource
  { load(ctx) }` keyed by the action id of a layout menu item;
  `Feature.menuSources`, merged by the central registry
  (`AppRegistry.menuSources`, a duplicate fails like an action);
  `menuSourceFor`; `literalMenuLabel(text)` for labels that are user data
  (identity until the menus read an escape). The context menu showing a
  source as a hover submenu is the R1 leftovers package's.
- **Mock core:** `GetMessageFacts` (`features/rules/fromMessage/mock.ts`,
  from the summary and the mock body: no raw headers, so no `List-Id`),
  `SuggestValues` (`ui/src/api/mockValues.ts`: tags, the mock's header names,
  no mailing lists), `RuleFromText` drafts (only the empty name),
  `RuleRun.target`, runs deleted with their rule. The mock's `RunRules` still
  answers `running` runs (the R1 leftovers package aligns it).
- **Theme regions** (`defaults/theme.kdl`) so the packages do not collide:
  `rules.header`, `rules.header.name`, `rules.segmented`,
  `rules.segmented.option`, `rules.form`, `rules.form.section`,
  `rules.form.label`, `rules.form.body`, `rules.lower`, `rules.lower.tabs`,
  `rules.lower.tab`, `rules.lower.panel`, `rules.footer`,
  `rules.footer.status`, `rules.footer.start`, `rules.footer.end`,
  `rules.empty`; `picker`, `picker.input`. The rules window stays one
  `rule-manager` widget in `layout.kdl`; its size (the review's 1100×760 or a
  share of the screen) is the rules package's change.

### Packages (`docs/plan.md`, Wave R2)

- **WR3 rules dialog redesign:** `ui/src/features/rules/` except
  `fromMessage/` (and `mock.ts`'s `RunRules`), the badges in
  `ui/src/features/exchange/Rules.tsx`, the rules window in `layout.kdl`,
  theme parts `rules.*`, `locales/en/rules.ftl`, `userDocs/en/rules.md`, the
  rules e2e scenarios.
- **WR4 shared pickers:** `ui/src/widgets/pickers/`, `ui/src/api/mockValues.ts`,
  their use outside the rules dialog (virtual folder editor, search, settings),
  `locales/en/pickers.ftl`, theme parts `picker.*`, picker e2e scenarios. In
  the rules dialog WR3 places the `Picker`.
- **WR5 values from the mail:** `ui/src/features/rules/fromMessage/`,
  `crates/nail-core/src/values.rs` if the facts need more, the entry points
  that open the window from a message (they set `origin`), its e2e scenario.
- **WR6 R1 leftovers:** `ui/src/features/menus/` (the hover submenu of a
  source, `literalMenuLabel`), "Run rule" moved onto a source
  (`features/rules/runMenu.ts` and its registration in the rules
  `index.ts`), the search hints overlay and the folder-change re-run in
  `widgets/SearchBar.tsx` / `features/search/` / the search state, the mock's
  `RunRules` in `features/rules/mock.ts`.

## Consequences

- The redesigned editor can switch views on an unfinished rule and show its
  gaps as problems; Recent runs can say when and on what a run acted; a
  re-created rule starts with an empty history.
- One picker component and one set of parts serve every value field; new
  kinds are a new source, not a new widget.
- `SuggestValues` reads up to 200 raw messages for mailing lists and header
  names: fast enough for a picker that opens on demand; an index of
  `List-Id`s is a later option if it is not.
- Several packages change `defaults/layout.kdl` / `theme.kdl`: the
  integrator regenerates the snapshots once (`NAIL_UPDATE_SNAPSHOTS=1` for
  `nail-config` and the `nail-api` menus vectors, `NAIL_UPDATE_GOLDEN=1`).
