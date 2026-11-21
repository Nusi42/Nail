/**
 * The shared pickers' contract (R2, ADR 0222). One generic `Picker`
 * component serves every field that names a folder, an account, a tag, an
 * address or contact, a mailing list or a header name; a *source* per kind
 * supplies its options. Free text always stays allowed: the picker only
 * helps to find a value, it never forces one from its list.
 */
import type { Account, AccountId, Address, Folder } from "../../api/generated";

/** What a picker picks. */
export const PICKER_KINDS = [
  "folder",
  "account",
  "tag",
  "address",
  "mailing-list",
  "header-name",
] as const;

export type PickerKind = (typeof PICKER_KINDS)[number];

/** What a folder picker's value holds: the folder's path (rules) or its id. */
export type FolderValue = "path" | "id";

/** One option of a picker's list. */
export interface PickerOption {
  /**
   * The row's key when `value` is not unique in its list (a folder path in
   * two accounts); `parent` refers to it. Defaults to `value`.
   */
  key?: string;
  /** The value the field takes when the option is picked. */
  value: string;
  /** What the list shows (already localized where it needs to be). */
  label: string;
  /** A second line or column (an address under a name, a count, an account), else `null`. */
  detail: string | null;
  /** Tree depth (folders; 0 = top level). */
  depth: number;
  /** The key of the option's parent in a tree (folders), else `null`. */
  parent: string | null;
  /** The folder of a folder option. */
  folder?: Folder;
  /** The address of an address option. */
  address?: Address;
  /** The account of an account option, or of a folder tree's account row. */
  account?: Account;
  /**
   * `group`: a heading row of a tree (an account above its folders) that
   * only opens or closes, never becomes the value; `create`: the row that
   * takes the typed text as a new value ("Create tag …").
   */
  variant?: "group" | "create";
}

/** What a source needs besides the typed text. */
export interface PickerScope {
  /** Folder pickers: the account whose folders are offered; `null` = every account. */
  accountId: AccountId | null;
  /** Folder pickers: what the value holds (default `path`). */
  folderValue: FolderValue;
  /** Account pickers: offer virtual accounts too (default: real accounts only). */
  virtualAccounts?: boolean;
}

/** The options of one picker for the text typed so far. */
export interface PickerOptions {
  options: readonly PickerOption[];
  /** Still loading (the list shows a placeholder, the field stays usable). */
  loading: boolean;
}

export interface PickerProps {
  kind: PickerKind;
  /** The field's value (free text or a picked option's value). */
  value: string;
  /** Called on typing (`option` `null`) and on picking an option. */
  onChange: (value: string, option: PickerOption | null) => void;
  /** Accessible name of the field (localized). */
  label: string;
  /** Folder pickers: the account whose folders are offered; `null` / omitted = every account. */
  accountId?: AccountId | null | undefined;
  /** Folder pickers: what `value` holds (default `path`). */
  folderValue?: FolderValue | undefined;
  /** Account pickers: offer virtual accounts too (default: real accounts only). */
  virtualAccounts?: boolean | undefined;
  /** The input's id (a host's `<label htmlFor>`). */
  id?: string | undefined;
  /** Localized placeholder. */
  placeholder?: string | undefined;
  disabled?: boolean | undefined;
  /** Marks the field invalid (`aria-invalid`). */
  invalid?: boolean | undefined;
  /** Ids of elements that describe the field (`aria-describedby`; a problem badge). */
  describedBy?: string | undefined;
  /** Extra `data-part` of the root for the host's styling (`rules.condition.value`), else `picker`. */
  part?: string | undefined;
}
