/**
 * Where the shared pickers' options come from (R2, ADR 0222), by kind:
 *
 * - `folder`: `ListFolders` (the query `useFolders` reads) of one account,
 *   or of every real account as a tree under account rows; filtered by name
 *   or path, a match keeps its parents.
 * - `account`: `ListAccounts` (`useAccounts`).
 * - `address`: `SuggestAddresses`: contacts, collected addresses and recent
 *   correspondents (the core ranks them), matching a name or an address;
 *   with nothing typed the most-used correspondents (R3, ADR 0227).
 * - `tag`, `mailing-list`, `header-name`: `SuggestValues` (`suggestValues`
 *   below): tags in use (plus "Create tag …" for a new one), the `List-Id`s
 *   and header names of recent mail (header names after the common ones).
 *
 * The option builders are pure functions (tested on their own);
 * `usePickerOptions` asks the core and feeds them.
 */
import { useQueries, useQuery } from "@tanstack/react-query";
import type { NailClient } from "../../api/client";
import type {
  Account,
  AccountId,
  Address,
  Folder,
  Identity,
  SuggestKind,
  ValueSuggestion,
} from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { TYPING_PAUSE_MS, useDebounced } from "../../state/debounce";
import { foldersQueryOptions, useAccounts, useClient } from "../../state/queries";
import { filterFolders } from "../FolderPicker";
import { flattenFolders } from "../FolderTree";
import { folderRoleLabelId } from "../folderNames";
import type { FolderValue, PickerKind, PickerOption, PickerOptions, PickerScope } from "./types";

/** How many values a `SuggestValues` picker asks for. */
export const PICKER_SUGGESTIONS = 50;

/** How many addresses an address picker asks for. */
export const PICKER_ADDRESSES = 20;

/**
 * Header names offered after the ones seen in mail: the headers rules and
 * searches ask about most (in this order).
 */
export const COMMON_HEADER_NAMES = [
  "From",
  "To",
  "Cc",
  "Reply-To",
  "Sender",
  "Subject",
  "List-Id",
  "Return-Path",
  "X-Spam-Status",
] as const;

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

/** The row key of an option (unique in its list; `parent` names it). */
export function optionKey(option: PickerOption): string {
  return option.key ?? option.value;
}

function contains(text: string, needle: string): boolean {
  return text.toLowerCase().includes(needle);
}

/** What the folder tree is built from. */
export interface FolderTreeInput {
  /** The accounts whose folders are offered, in order. */
  accounts: readonly Account[];
  /** Their folders (an account without an entry has none loaded yet). */
  folders: ReadonlyMap<AccountId, readonly Folder[]>;
  query: string;
  folderValue: FolderValue;
  /** Account rows above the folders (a picker over several accounts). */
  grouped: boolean;
  /** The localized name of a special-use folder's role, `null` for other folders. */
  roleLabel: (folder: Folder) => string | null;
}

/** The folders that match `query` and their parents (ids). */
function foldersWithParents(folders: readonly Folder[], query: string): Set<string> {
  const byId = new Map(folders.map((f) => [f.id, f]));
  const keep = new Set<string>();
  for (const match of filterFolders(folders, query)) {
    let folder: Folder | undefined = match;
    while (folder && !keep.has(folder.id)) {
      keep.add(folder.id);
      folder = folder.parent_id === null ? undefined : byId.get(folder.parent_id);
    }
  }
  return keep;
}

/**
 * The folder tree: per account (a `group` row when `grouped`) its folders
 * in the folder pane's order (special-use folders first); with a query only
 * the matching folders and their parents.
 */
export function folderTreeOptions(input: FolderTreeInput): PickerOption[] {
  const out: PickerOption[] = [];
  for (const account of input.accounts) {
    const folders = input.folders.get(account.id) ?? [];
    const ids = new Set(folders.map((f) => f.id));
    const keep = foldersWithParents(folders, input.query);
    const rows = flattenFolders(folders, { rolesFirst: true }).filter((r) => keep.has(r.folder.id));
    if (rows.length === 0) continue;
    const groupKey = `account:${account.id}`;
    if (input.grouped) {
      out.push({
        key: groupKey,
        value: account.id,
        label: account.name,
        detail: account.email,
        depth: 0,
        parent: null,
        account,
        variant: "group",
      });
    }
    const shift = input.grouped ? 1 : 0;
    for (const { folder, depth } of rows) {
      const role = input.roleLabel(folder);
      const top = input.grouped ? groupKey : null;
      out.push({
        key: `folder:${folder.id}`,
        value: input.folderValue === "id" ? folder.id : folder.path,
        label: folder.name,
        detail: role !== null && role !== folder.name ? role : null,
        depth: depth + shift,
        parent:
          folder.parent_id !== null && ids.has(folder.parent_id)
            ? `folder:${folder.parent_id}`
            : top,
        folder,
      });
    }
  }
  return out;
}

/** Accounts whose name or address contains the query. */
export function accountOptions(accounts: readonly Account[], query: string): PickerOption[] {
  const needle = query.trim().toLowerCase();
  return accounts
    .filter((a) => contains(a.name, needle) || contains(a.email, needle))
    .map((account) => ({
      value: account.id,
      label: account.name,
      detail: account.email,
      depth: 0,
      parent: null,
      account,
    }));
}

/** Addresses as options: the name with the address beside it (the address alone without one). */
export function addressOptions(addresses: readonly Address[]): PickerOption[] {
  return addresses.map((address) => ({
    value: address.email,
    label: address.name ?? address.email,
    detail: address.name === null ? null : address.email,
    depth: 0,
    parent: null,
    address,
  }));
}

/** The addresses of an account's sending identities (a picker's `ownAddresses`). */
export function identityAddresses(
  identities: readonly Pick<Identity, "name" | "email">[],
): Address[] {
  return identities.map(({ name, email }) => ({ name, email }));
}

/**
 * The user's own addresses whose name or address contains the typed text
 * (case-insensitive), each address once, as options.
 */
export function ownAddressOptions(own: readonly Address[], query: string): PickerOption[] {
  const needle = query.trim().toLowerCase();
  const seen = new Set<string>();
  const matching = own.filter((address) => {
    const key = address.email.toLowerCase();
    if (seen.has(key)) return false;
    seen.add(key);
    return (
      needle === "" ||
      key.includes(needle) ||
      (address.name?.toLowerCase().includes(needle) ?? false)
    );
  });
  return addressOptions(matching);
}

/**
 * `SuggestValues` answers as options. Header names: the seen ones, then the
 * common ones not seen (both matching the query's start). Tags: a `create`
 * row for a typed tag that is not in the list (`createLabel` names it).
 */
export function valueOptions(
  kind: SuggestKind,
  values: readonly ValueSuggestion[],
  query: string,
  createLabel: (value: string) => string,
): PickerOption[] {
  const typed = query.trim();
  const needle = typed.toLowerCase();
  const all: ValueSuggestion[] = [...values];
  if (kind === "header-name") {
    const seen = new Set(values.map((v) => v.value.toLowerCase()));
    for (const name of COMMON_HEADER_NAMES) {
      if (name.toLowerCase().startsWith(needle) && !seen.has(name.toLowerCase())) {
        all.push({ value: name, label: null, count: 0 });
      }
    }
  }
  const options: PickerOption[] = all.map((v) => ({
    value: v.value,
    label: v.label ?? v.value,
    detail: v.label === null ? null : v.value,
    depth: 0,
    parent: null,
  }));
  if (kind === "tag" && typed !== "" && !values.some((v) => v.value.toLowerCase() === needle)) {
    options.push({
      key: `create:${typed}`,
      value: typed,
      label: createLabel(typed),
      detail: null,
      depth: 0,
      parent: null,
      variant: "create",
    });
  }
  return options;
}

/** What `usePickerOptions` is asked for. */
export interface PickerQuery {
  kind: PickerKind;
  /** The text typed so far. */
  query: string;
  scope: PickerScope;
  /** Whether to ask the core at all (a closed list asks nothing; default `true`). */
  enabled?: boolean;
}

/** `accounts` limited to the scope's `accounts` when it names any. */
function inScope(accounts: readonly Account[], scope: PickerScope): Account[] {
  const only = scope.accounts;
  return only === undefined ? [...accounts] : accounts.filter((a) => only.includes(a.id));
}

/** The real accounts a folder picker offers: the scope's, or every real one. */
export function folderAccounts(accounts: readonly Account[], scope: PickerScope): Account[] {
  return scope.accountId === null
    ? inScope(accounts, scope).filter((a) => a.kind !== "virtual")
    : accounts.filter((a) => a.id === scope.accountId);
}

/** The accounts an account picker offers: real ones (and virtual ones when asked), in scope. */
export function offeredAccounts(accounts: readonly Account[], scope: PickerScope): Account[] {
  return inScope(accounts, scope).filter(
    (a) => scope.virtualAccounts === true || a.kind !== "virtual",
  );
}

/** The options of a picker for the text typed so far. */
export function usePickerOptions(request: PickerQuery): PickerOptions {
  const { kind, scope } = request;
  const enabled = request.enabled ?? true;
  const t = useT();
  const client = useClient();
  const query = useDebounced(request.query, TYPING_PAUSE_MS);
  const suggestKind = suggestKindOf(kind);
  const accounts = useAccounts();
  const accountList = accounts.data ?? [];
  const shown = kind === "folder" ? folderAccounts(accountList, scope) : [];
  const folderQueries = useQueries({
    queries: shown.map((account) => ({
      ...foldersQueryOptions(client, account.id),
      enabled,
    })),
  });
  const addresses = useQuery({
    queryKey: ["pickers", "addresses", query],
    queryFn: async () =>
      (
        await client.request(
          { type: "SuggestAddresses", prefix: query, limit: PICKER_ADDRESSES },
          "AddressSuggestions",
        )
      ).addresses,
    // An empty text asks too: the core answers the most-used addresses.
    // A field for the user's own addresses never asks (those are other people's).
    enabled: enabled && kind === "address" && scope.ownAddresses === undefined,
  });
  const values = useQuery({
    queryKey: ["pickers", "values", suggestKind, query],
    queryFn: () => suggestValues(client, suggestKind ?? "tag", query),
    enabled: enabled && suggestKind !== null,
  });
  if (!enabled) return { options: [], loading: false };
  switch (kind) {
    case "folder": {
      const folders = new Map<AccountId, readonly Folder[]>();
      shown.forEach((account, i) => {
        const data = folderQueries[i]?.data;
        if (data) folders.set(account.id, data);
      });
      return {
        options: folderTreeOptions({
          accounts: shown,
          folders,
          query,
          folderValue: scope.folderValue,
          grouped: scope.accountId === null && shown.length > 1,
          roleLabel: (folder) => {
            const id = folderRoleLabelId(folder);
            return id === null ? null : t(id);
          },
        }),
        loading: accounts.isLoading || folderQueries.some((q) => q.isLoading),
      };
    }
    case "account":
      return {
        options: accountOptions(offeredAccounts(accountList, scope), query),
        loading: accounts.isLoading,
      };
    case "address":
      return scope.ownAddresses === undefined
        ? { options: addressOptions(addresses.data ?? []), loading: addresses.isLoading }
        : { options: ownAddressOptions(scope.ownAddresses, query), loading: false };
    case "tag":
    case "mailing-list":
    case "header-name":
      return {
        options: valueOptions(kind, values.data ?? [], query, (tag) =>
          t("pickers-create-tag", { tag }),
        ),
        loading: values.isLoading,
      };
  }
}
