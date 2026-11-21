/**
 * The search field's value helpers (R2 shared pickers, ADR 0222): after a
 * field of the query language that names something (`from:`, `to:`, `cc:`,
 * `tag:`, `account:`, `folder:`) the search field offers the values the
 * pickers offer for that kind, filtered by what was typed after the colon;
 * picking one completes the word (quoted when it needs quotes).
 *
 * `searchValueContext` / `applySearchValue` are pure; `SearchValues` is the
 * list the search field shows under its hints (buttons, so Tab reaches them
 * like the hints; a mouse press keeps the focus in the field).
 */
import { useT } from "../../i18n/I18nProvider";
import { filterFolders } from "../FolderPicker";
import { Icon } from "../Icon";
import { optionKey, usePickerOptions } from "./sources";
import { pickable } from "./tree";
import type { PickerKind, PickerOption } from "./types";

/** Query-language fields whose value a picker offers, with the picker's kind. */
export const SEARCH_VALUE_FIELDS = {
  "from:": "address",
  "to:": "address",
  "cc:": "address",
  "tag:": "tag",
  "account:": "account",
  "folder:": "folder",
} as const satisfies Record<string, PickerKind>;

type SearchValueField = keyof typeof SEARCH_VALUE_FIELDS;

/** The value being typed after a field (`folder:Arc` → field `folder:`, prefix `Arc`). */
export interface SearchValueContext {
  field: SearchValueField;
  kind: PickerKind;
  /** The value typed so far (without an opening quote). */
  prefix: string;
  /** Where the value starts in the text (after the colon). */
  start: number;
}

function isField(word: string): word is SearchValueField {
  return Object.hasOwn(SEARCH_VALUE_FIELDS, word);
}

/** The field value being typed at the end of `text`, `null` when the last word is no such field. */
export function searchValueContext(text: string): SearchValueContext | null {
  // The last word, where a quoted value may hold spaces (`folder:"Old Pro`).
  const match = /(?:^|[\s(])(-?)([a-z]+:)("(?:[^"\\]|\\.)*|[^\s()"]*)$/i.exec(text);
  if (!match) return null;
  const field = (match[2] ?? "").toLowerCase();
  if (!isField(field)) return null;
  const raw = match[3] ?? "";
  const start = text.length - raw.length;
  const prefix = raw.startsWith('"') ? raw.slice(1).replace(/\\(.)/g, "$1") : raw;
  return { field, kind: SEARCH_VALUE_FIELDS[field], prefix, start };
}

/** A value as the query language reads it: quoted (with `\` escapes) when it holds spaces, quotes or brackets. */
export function searchValueText(value: string): string {
  return /[\s()"\\]/.test(value) ? `"${value.replace(/["\\]/g, "\\$&")}"` : value;
}

/** The value a search takes from an option: an address, a tag, an account's address, a folder's path. */
export function searchValueOf(option: PickerOption): string {
  if (option.account) return option.account.email;
  if (option.folder) return option.folder.path;
  return option.value;
}

/** `text` with the value being typed replaced by `value` and a space to go on. */
export function applySearchValue(text: string, context: SearchValueContext, value: string): string {
  return `${text.slice(0, context.start)}${searchValueText(value)} `;
}

/**
 * The options a search offers: no account rows, no "Create tag", no folder
 * that is only there as a match's parent, and every value once (a folder
 * path two accounts have is one row).
 */
function distinctValues(options: readonly PickerOption[], prefix: string): PickerOption[] {
  const seen = new Set<string>();
  return options.filter((o) => {
    if (!pickable(o) || o.variant === "create") return false;
    if (o.folder && filterFolders([o.folder], prefix).length === 0) return false;
    const value = searchValueOf(o).toLowerCase();
    if (seen.has(value)) return false;
    seen.add(value);
    return true;
  });
}

/** How many values the search field shows at most. */
export const SEARCH_VALUES_SHOWN = 8;

export interface SearchValuesProps {
  /** The search field's text. */
  text: string;
  /** Called with the new text when a value is picked. */
  onApply: (text: string) => void;
}

/** The values for the field being typed at the end of the search text (nothing for other words). */
export function SearchValues({ text, onApply }: SearchValuesProps) {
  const t = useT();
  const context = searchValueContext(text);
  const { options } = usePickerOptions({
    kind: context?.kind ?? "tag",
    query: context?.prefix ?? "",
    scope: { accountId: null, folderValue: "path", virtualAccounts: true },
    enabled: context !== null,
  });
  if (context === null) return null;
  const shown = distinctValues(options, context.prefix).slice(0, SEARCH_VALUES_SHOWN);
  if (shown.length === 0) return null;
  return (
    <ul data-part="picker.list" data-kind={context.kind} aria-label={t("pickers-search-values")}>
      {shown.map((option) => (
        <li key={optionKey(option)} data-part="picker.option" data-depth={0}>
          <button
            type="button"
            data-part="picker.option.button"
            // Keep the focus in the search field while picking.
            onMouseDown={(event) => {
              event.preventDefault();
            }}
            onClick={() => {
              onApply(applySearchValue(text, context, searchValueOf(option)));
            }}
          >
            {option.folder && (
              <Icon
                name={option.folder.role === "other" ? "folder" : option.folder.role}
                part="picker.option.icon"
              />
            )}
            <span data-part="picker.option.label">
              {option.folder ? option.folder.path : option.label}
            </span>
            {option.detail !== null && (
              <span data-part="picker.option.detail">{option.detail}</span>
            )}
          </button>
        </li>
      ))}
    </ul>
  );
}
