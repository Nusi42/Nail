# 0223 — WR4: the shared pickers

Status: accepted (2026-11-21)

## Context

ADR 0222 defined one generic `Picker` (`ui/src/widgets/pickers/`) for every
field that names a folder, an account, a tag, an address or contact, a
mailing list or a header name, with a source per kind, and left the list,
the folder tree, the keyboard model and the sources to this package. The
maintainer asked for browse / search / select everywhere such fields exist,
folders as a searchable tree, free text still allowed.

## Decision

- **ARIA combobox.** The input is the `combobox` (`aria-expanded`,
  `aria-controls`, `aria-autocomplete="list"`, `aria-activedescendant`);
  the popup is a `listbox`, or a `tree` with `treeitem`s (`aria-level`,
  `aria-expanded`) for folders. The focus never leaves the input: a mouse
  press on the browse button or a row is prevented, the keys act on the
  active row. Keys: Down / Alt+Down open (showing every choice) and move, Up
  moves back, Alt+Up closes, Home / End while browsing, Right opens a row
  with children or enters it, Left closes it or goes to its parent, Enter
  picks (an account row of the folder tree only opens / closes), Escape
  closes without letting the window see the key, Tab closes. A mouse press
  outside the picker closes it, as does losing the focus.
- **Two lists.** Browsing (the `▾` button, Down) shows every choice and
  starts on the field's value; typing filters (debounced like the
  composer's address suggestions) and starts on the first row whose name
  starts with the typed text, so Enter right after typing does not pick a
  row of the not yet filtered list.
- **Free text.** Typing always calls `onChange(text, null)`. Fields whose
  value is an id (accounts; folders with `folderValue="id"`) show the
  option's name and, when the field loses the focus, take a typed name that
  matches exactly one option as that option. Hosts that need a real choice
  (the folder policies, the notification overrides) act only on a picked
  option.
- **Sources** (`sources.ts`, pure builders plus `usePickerOptions`):
  folders from `ListFolders` per account (the same query keys as
  `useFolders`), with account rows when a picker spans several accounts
  (`accountId: null`: every real account), the special-use folders first,
  a match keeping its parents, the role beside a folder whose name differs;
  accounts from `ListAccounts` (real accounts unless `virtualAccounts`);
  addresses from `SuggestAddresses` (the core already ranks contacts,
  collected addresses and recent correspondents, so no new request);
  tags / mailing lists / header names from `SuggestValues`, tags with a
  "Create tag …" row, header names followed by a fixed list of common ones
  (`COMMON_HEADER_NAMES`). No core request was missing.
- **Row identity.** `PickerOption` gains `key` (a folder path is not unique
  across accounts; `parent` names a key), `account` and `variant`
  (`group` | `create`).
- **Overlay.** `picker.field` is the positioned box; `picker.popup` is
  absolutely positioned under the input with a bounded height, so opening
  the list never moves the content below; with less room below the field
  than the list's tallest size and more above, it opens above
  (`data-placement="above"`). Indentation per depth and the
  highlighted / selected rows are theme rules (`picker.option[depth=…]`,
  state `highlighted`, condition `selected`) using the colour tokens, so dark
  mode and high contrast follow the theme.
- **Forms.** `PickerField` puts a picker in the shared `form.field` frame
  (`LabelledField`). Used for: the folder sources of a virtual account
  (account + folder path), the account and folder of the synchronization
  policies, the folder of a new notification override, an identity's
  reply-to address and the vacation reply's sender. Account choosers that
  are already selects over a handful of accounts (junk settings, search
  index, Sieve, MCP grants) stay selects; lists of several addresses in one
  field (vacation "Also my addresses") stay text.
- **Search field.** `searchValues.tsx` holds the helpers for the values
  after `from:` / `to:` / `cc:` / `tag:` / `account:` / `folder:`
  (`searchValueContext`, `applySearchValue`, quoting with `\` escapes) and
  the `SearchValues` list. The search field (`widgets/SearchBar.tsx`) belongs
  to the R1 leftovers package in this wave; the integrator mounts the list
  under its hints.
- **Mock.** `SuggestValues` for `mailing-list` answers the mock's
  mailing-list post (`List-Id` derived from its list address, named after
  its sender).

## Consequences

- One component and one set of parts serve the rules editor and every
  settings field above; a new kind is a new source.
- Pickers on a settings page ask for accounts and folders only when they
  show an id value or are opened.
- The skin (`theme/skin.ts`) styles `form.input` but not yet `picker.input`;
  the theme's `picker.input` rule matches its looks.
