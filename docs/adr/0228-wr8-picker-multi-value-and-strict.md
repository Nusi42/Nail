# 0228 — Pickers everywhere: multi-value mode, strict fields, account scope

Status: accepted (2026-11-21)

## Context

ADR 0227 (decisions 4 and 5) moves every remaining plain field that names a
folder, an account or an address to the shared `Picker` (ADR 0222/0224) and
lets an empty address picker list the most-used addresses. Several of those
fields hold a list (the vacation reply's "Also my addresses", an MCP
client's accounts and hidden folders, the accounts whose decrypted mail is
indexed, the folders to export), others choose which account a settings
form is for, where free text means nothing.

## Decision

- **Multi-value mode in the shared `Picker`** (`multiple: { values,
  onChange }`, labelled form `PickerListField`), not a second component.
  Chosen values are chips before the input (`picker.chips`, `picker.chip`,
  `picker.chip.label`, `picker.chip.remove`), each with a remove button.
  The list is `aria-multiselectable`; picking an option adds it, picking a
  chosen one removes it, and the list stays open. Typed text becomes a value
  on Enter, `,` / `;` (also in pasted text) and on leaving the field — for
  id kinds (accounts, folders by id) only text that names one option.
  Backspace in the empty field removes the last value. Values compare
  case-insensitively and keep their first spelling (`multiple.ts`). Chips
  show a folder's path, another option's name, else the value.
- **Strict fields** (`strict`): only a picked option or an empty text
  changes the value; other typed text stays in the field until it is left,
  then the field shows its value again (or takes the one option the text
  names). Used for "which account is this form for" fields and the import
  target, where the empty value has a meaning shown as the placeholder
  ("Local Folders", "The Inbox").
- **Account scope** (`accounts`): account and folder pickers can offer only
  some accounts (IMAP accounts for Sieve, the granted accounts for MCP's
  hidden folders, the Exchange accounts of automatic replies).
- **Empty address picker:** the address source asks `SuggestAddresses` also
  with an empty text (the contract's most-used correspondents); the empty
  message now says no address was used yet.
- Converted: junk settings' account, Sieve and vacation account, "Also my
  addresses", the vacation sender (already a picker), the accounts with
  searchable decrypted mail (were check boxes), MCP accounts and hidden
  folders (were check boxes per account), the export's account and folders
  (a select and check boxes), the import's target account and folder
  (selects), the key lookup's address, the automatic replies' account.
- Left as they are: the import preview's folders (folders of the source
  file, not of Nail), calendars, address books and task lists (no picker
  kind), the composer's From (identities), Exchange room lists and shared
  mailbox addresses, Gmail's parent label, plug-in folder pickers (their
  parts belong to the plug-in UI contract), the scheduling attendee field
  (has its own suggestions).

## Consequences

- New theme parts `picker.chips`, `picker.chip`, `picker.chip.label`,
  `picker.chip.remove`, `picker.chip.remove.icon` (styled in
  `defaults/theme.kdl`, listed in the theme reference); the MCP grant's
  `mcp-grants.folders.*` parts are gone.
- Hosts keep a list in their own state and pass it as `values`; no host
  parses comma-separated text any more (the sieve feature's own address
  list parser is removed).
