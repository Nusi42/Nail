/**
 * Where the shared pickers' options come from (R2, ADR 0222), by kind:
 *
 * - `folder`: `ListFolders` (`useFolders`), as a tree; filtered by name or path.
 * - `account`: `ListAccounts` (`useAccounts`).
 * - `address`: `SuggestAddresses` (collected addresses) and contacts.
 * - `tag`, `mailing-list`, `header-name`: `SuggestValues` (`suggestValues`
 *   below): tags in use, the `List-Id`s and header names of recent mail.
 *
 * Contract stub: `usePickerOptions` offers nothing yet; the pickers
 * package (WR4) fills it in.
 */
import type { NailClient } from "../../api/client";
import type { SuggestKind, ValueSuggestion } from "../../api/generated";
import type { PickerKind, PickerOptions, PickerScope } from "./types";

/** How many values a `SuggestValues` picker asks for. */
export const PICKER_SUGGESTIONS = 50;

/** The `SuggestValues` kind of a picker kind, `null` for kinds with their own request. */
export function suggestKindOf(kind: PickerKind): SuggestKind | null {
  switch (kind) {
    case "tag":
      return "tag";
    case "mailing-list":
      return "mailing-list";
    case "header-name":
      return "header-name";
    case "folder":
    case "account":
    case "address":
      return null;
  }
}

/** Tags, mailing lists or header names that start with `prefix`, most used first. */
export async function suggestValues(
  client: NailClient,
  kind: SuggestKind,
  prefix: string,
  limit = PICKER_SUGGESTIONS,
): Promise<ValueSuggestion[]> {
  return (await client.request({ type: "SuggestValues", kind, prefix, limit }, "ValueSuggestions"))
    .values;
}

/** What `usePickerOptions` is asked for. */
export interface PickerQuery {
  kind: PickerKind;
  /** The text typed so far. */
  query: string;
  scope: PickerScope;
}

/** The options of a picker for the text typed so far (stub: none yet). */
// Contract stub (ADR 0222): the pickers package (WR4) implements it.
export const usePickerOptions: (request: PickerQuery) => PickerOptions = () => ({
  options: [],
  loading: false,
});
