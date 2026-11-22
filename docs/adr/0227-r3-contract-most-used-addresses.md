# 0227 — R3 contract: most-used addresses, rules window decisions, Paper theme

Status: accepted (2026-11-21)

## Context

The maintainer tried the 0.8.7 build (waves R1 and R2) and decided the next
steps on 2026-11-21:

1. One combined Exchange badge per rule row: one ⓘ with the total, its hover
   / focus text lists every account's reason (R2 shows one badge per account).
2. The "Values from this mail" button directly after a condition's text field,
   before the case checkbox.
3. Multi-select in the rule list (Ctrl / Shift-click, Ctrl+A, Shift+arrow
   keys, `aria-multiselectable`); the footer's Run runs the selected rules in
   file order on the Run on target through the existing `RunRules`.
4. Every remaining plain dropdown or text field naming a folder, account or
   address uses the shared `Picker` (`ui/src/widgets/pickers/`): junk
   settings, search index, Sieve, the vacation account, MCP grants, import /
   export folders, "Also my addresses" (several addresses).
5. The address picker lists the most-used addresses while its field is empty.
6. A new built-in light theme, Paper.

0.8.7 was prepared but not published (no tag); the next release is 0.8.8 and
contains R1, R2 and R3.

## Decision

### Most-used addresses (core, this contract)

- `SuggestAddresses { prefix, limit }` keeps its shape. An empty or blank
  `prefix` now answers the most-used correspondents instead of nothing:
  `Store::most_used_recipients(limit, now, exclude)` ranks the collected
  addresses (`collected_addresses`: every address of sent and received mail)
  by the same usage score as typed suggestions (`usage_score`: frequency
  fading over months, recency over weeks), leaves out the user's own
  addresses (`Service::own_addresses`: accounts and identities,
  case-insensitive) and carries a contact's name when the address belongs to
  a contact in an address book with autocomplete on. The limit is the usual
  one (1–50, `MAX_SUGGESTIONS`).
- Rejected: a new request (`ListFrequentAddresses`). The picker asks one
  source for every text it has; an empty text is just the shortest prefix, and
  the composer never asks with an empty prefix (its field queries only after a
  typed character), so nothing else changes behaviour.
- Contacts that were never written to or received from are not listed: the
  list is "who you correspond with", not the address book.
- The limited remote access answers an empty list as before (no change).
- The mock (`ui/src/features/composer/mock.ts` `suggest`) answers the mock's
  collected addresses by use (own identities left out), cut to the limit.
- The store's candidate row cap (500 per source) is one constant,
  `CANDIDATE_ROWS`, for both queries.

### Rules window decisions (WR7 builds them)

- **Exchange badge:** one ⓘ per rule row with the summed count; the hover /
  focus text has one line per account and reason.
- **Values button:** in the condition row's order: field, values button, case
  checkbox.
- **Multi-select:** the list is a `listbox` with `aria-multiselectable="true"`;
  Click selects one, Ctrl+click toggles, Shift+click and Shift+Up / Down
  extend from the anchor, Ctrl+A selects all (only while the list has focus).
  The editor shows the focused rule (the last one clicked or moved to).
- **Run with several rules:** the footer's Run sends one `RunRules` with the
  selected ids on the Run on target (the core runs them in file order, each
  its own run and undo). Its label names the number ("Run 3 rules"). Unsaved
  changes to the shown rule ask first, as for one rule. A destructive run asks
  with the total preview count.
- **Test / Preview with several rules:** Test tests each selected rule on its
  own (one `TestRule` per rule, same target) and the Preview tab groups the
  results by rule in file order. It does not simulate the rules one after
  another (a move by an earlier rule does not hide the message from a later
  one in the preview); the Preview says so in one line. "Apply to these"
  applies only within one rule's group.
- Save, Delete and the Text view act on the shown rule only.

### Pickers everywhere (WR8 builds them)

- Every listed field uses `Picker` / `PickerField`; free text stays allowed
  where the field allowed it before. A field holding several addresses ("Also
  my addresses") uses the picker's multi-address mode (chips plus the input,
  WR8 adds it to `ui/src/widgets/pickers/`).
- The address source asks `SuggestAddresses` also for an empty text (the
  most-used list above) when the picker opens.

### Paper theme (WR9 builds it)

- `defaults/themes/paper.kdl`, registered in `nail-config` `themes.rs`
  (`BUILTIN_THEMES`, then `npm run gen:api`), a light theme. It uses Nail's
  existing token names only; no new token is added just for this theme.
  It passes the same contrast tests as the other built-in themes and gets
  visual baselines.

### Release

- The unpublished 0.8.7 section of the release notes becomes "Nail 0.8.8";
  `CHANGELOG.md` keeps its entries under `[Unreleased]` until
  `release:prepare --version 0.8.8` moves them. The workspace version stays
  0.8.7 in the files until then.

## Consequences

- An address picker can show useful choices on focus; the composer is
  unchanged.
- Callers that relied on an empty prefix answering nothing (none in Nail) now
  get up to `limit` addresses.
- No new crates, no new dependencies, no schema change.
