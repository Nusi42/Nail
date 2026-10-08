/**
 * The message list header (`list-header`, M5b WP162, ADR 0109 /
 * 0111): the search field with the key that focuses it, the title of
 * what the list shows with its counts ("All inboxes — 11 unread · 3
 * accounts"), and the filter chips All / Unread / Flagged / Attachments,
 * which add a term of the query language (`./listFilter.ts`,
 * `UiState.listFilter`).
 *
 * Options: `search`, `hint`, `title`, `counts` (all default #true), `chips`
 * (default "all unread flagged attachments").
 */
import { folderRoleLabelId } from "./folderNames";
import {
  LIST_FILTER_MESSAGE_IDS,
  type Account,
  type AccountId,
  type Folder,
  type ListFilter,
} from "../api/generated";
import { ariaShortcut, keyLabel } from "../features/menus";
import { useLabel, useT } from "../i18n/I18nProvider";
import { useLayoutConfig } from "../layout/LayoutContext";
import { optionBool, type WidgetProps } from "../layout/widgetRegistry";
import { useAccounts, useFolders } from "../state/queries";
import { useUi } from "../state/uiStore";
import { ALL_ACCOUNTS } from "./AccountSwitcher";
import { wordsOption } from "./folderPane";
import { LIST_FILTERS } from "./listFilter";
import { SearchForm } from "./SearchBar";
import type { ResolvedLayout } from "../api/generated";

/** Unified folders titled like their favorite. */
const UNIFIED_TITLES: Readonly<Record<string, string>> = {
  inbox: "favorite-folder-all-inboxes",
  drafts: "favorite-folder-drafts",
  sent: "favorite-folder-sent",
};

/**
 * The key that focuses the search: a chord with a modifier (Ctrl+K) before
 * a single key ("/"), the last such binding of `search.focus` winning.
 */
export function searchShortcut(layout: ResolvedLayout): string | null {
  const keys = layout.keys
    .filter((k) => k.action === "search.focus" && k.args.length === 0)
    .map((k) => k.key);
  const chords = keys.filter((k) => /^(ctrl|alt|meta)\+/.test(k));
  return chords[chords.length - 1] ?? keys[keys.length - 1] ?? null;
}

/**
 * The Fluent id (or literal folder name) of the list's title: the
 * favorite's name for a unified folder of "All accounts", the role's name
 * for a special-use folder, else the folder's own name; `null` without a folder.
 */
export function folderTitle(
  folder: Folder | undefined,
  accountId: AccountId | null,
): { id: string } | { text: string } | null {
  if (!folder) return null;
  const unified = Object.hasOwn(UNIFIED_TITLES, folder.role)
    ? UNIFIED_TITLES[folder.role]
    : undefined;
  if (accountId === ALL_ACCOUNTS && folder.parent_id === null && unified) return { id: unified };
  const role = folderRoleLabelId(folder);
  return role ? { id: role } : { text: folder.name };
}

/** How many real accounts a unified view covers (`null` for one account). */
export function accountCount(
  accountId: AccountId | null,
  accounts: readonly Account[],
): number | null {
  if (accountId !== ALL_ACCOUNTS) return null;
  return accounts.filter((a) => a.kind !== "virtual").length;
}

export function ListHeader({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const layout = useLayoutConfig();
  const showSearch = optionBool(node, "search", true);
  const showHint = optionBool(node, "hint", true);
  const showTitle = optionBool(node, "title", true);
  const showCounts = optionBool(node, "counts", true);
  const chips = wordsOption<ListFilter>(node.options.chips, LIST_FILTERS, LIST_FILTERS);
  const accountId = useUi((s) => s.activeAccountId);
  const folderId = useUi((s) => s.activeFolderId);
  const search = useUi((s) => s.search);
  const filter = useUi((s) => s.listFilter);
  const setListFilter = useUi((s) => s.setListFilter);
  const folders = useFolders(accountId).data;
  const accounts = useAccounts().data ?? [];
  const folder = folders?.find((f) => f.id === folderId);
  const binding = showHint ? searchShortcut(layout) : null;

  let title: string | null = null;
  if (search !== null) {
    title = t("list-header-title-search");
  } else {
    const named = folderTitle(folder, accountId);
    if (named) title = "id" in named ? t(named.id) : named.text;
  }
  let counts: string | null = null;
  if (search === null && folder) {
    const unread = t("list-header-unread", { count: folder.unread });
    const covered = accountCount(accountId, accounts);
    counts =
      covered !== null && covered > 1
        ? t("list-header-counts", {
            unread,
            accounts: t("list-header-accounts", { count: covered }),
          })
        : unread;
  }

  return (
    <div
      data-part="list-header"
      data-node-id={node.id ?? undefined}
      role="region"
      aria-label={node.label ? label(node.label) : t("list-header-label")}
    >
      {showSearch && (
        <SearchForm
          label={t("search-bar-label")}
          scope="account"
          showHints
          liveCheck
          shortcut={binding === null ? null : keyLabel(binding, t)}
          keyShortcuts={binding === null ? undefined : ariaShortcut(binding)}
          placeholder={t("search-bar-input")}
        />
      )}
      {showTitle && title !== null && (
        <div data-part="list-header.heading">
          <h2 data-part="list-header.title">{title}</h2>
          {showCounts && counts !== null && <span data-part="list-header.counts">{counts}</span>}
        </div>
      )}
      {chips.length > 0 && (
        <div data-part="list-header.chips" role="group" aria-label={t("list-header-filters")}>
          {chips.map((chip) => (
            <button
              key={chip}
              type="button"
              data-part="chip"
              data-filter={chip}
              aria-pressed={filter === chip}
              onClick={() => {
                setListFilter(chip);
              }}
            >
              {t(LIST_FILTER_MESSAGE_IDS[chip])}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
