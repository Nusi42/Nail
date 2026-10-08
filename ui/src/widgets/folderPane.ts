/**
 * The folder pane's sections (M5b, ADR 0109) — the port of
 * `nail_api::views::folder_pane_sections`, pinned by
 * `crates/nail-api/tests/ts_vectors.rs` (vectors run in `layout/views.test.ts`):
 *
 * - sections in the order of the `folder-tree sections=` option (default
 *   `favorites accounts smart-folders`);
 * - Favorites only when it lists a folder (`favorites=`, default
 *   `all-inboxes flagged snoozed drafts`);
 * - one section per account, in account order, with a colour slot
 *   (1–8, the avatar palette `color.avatar-<n>`);
 * - smart folders only when there is one;
 * - collapsed sections come from the UI state
 *   (`folder-pane.collapsed`, `state/uiState.ts`).
 */
import {
  ACCOUNT_COLOR_SLOTS,
  type AccountId,
  type FavoriteFolder,
  FAVORITES_SECTION_ID,
  type FolderPaneSectionKind,
  SMART_FOLDERS_SECTION_ID,
} from "../api/generated";
export {
  ACCOUNT_COLOR_SLOTS,
  DEFAULT_FAVORITES,
  FAVORITES_SECTION_ID,
  FAVORITE_FOLDER_VALUES as FAVORITE_FOLDERS,
  FOLDER_PANE_SECTION_KIND_VALUES as FOLDER_SECTION_KINDS,
  SMART_FOLDERS_SECTION_ID,
} from "../api/generated";

/** `account_color_slot`: the 1-based colour slot of the account at `index`. */
export function accountColorSlot(index: number): number {
  return (index % ACCOUNT_COLOR_SLOTS) + 1;
}

/** `account_section_id`: the section id of an account (`account:<id>`). */
export function accountSectionId(account: AccountId): string {
  return `account:${account}`;
}

/** One section of the folder pane. */
export interface FolderPaneSection {
  id: string;
  kind: FolderPaneSectionKind;
  accountId: AccountId | null;
  colorSlot: number | null;
  collapsed: boolean;
}

/** `folder_pane_sections`. */
export function folderPaneSections(
  kinds: readonly FolderPaneSectionKind[],
  favorites: readonly FavoriteFolder[],
  accounts: readonly AccountId[],
  smartFolders: number,
  collapsed: readonly string[],
): FolderPaneSection[] {
  const out: FolderPaneSection[] = [];
  const isCollapsed = (id: string) => collapsed.includes(id);
  for (const kind of kinds) {
    if (kind === "favorites" && favorites.length > 0) {
      out.push({
        id: FAVORITES_SECTION_ID,
        kind,
        accountId: null,
        colorSlot: null,
        collapsed: isCollapsed(FAVORITES_SECTION_ID),
      });
    } else if (kind === "accounts") {
      accounts.forEach((account, index) => {
        const id = accountSectionId(account);
        out.push({
          id,
          kind,
          accountId: account,
          colorSlot: accountColorSlot(index),
          collapsed: isCollapsed(id),
        });
      });
    } else if (kind === "smart-folders" && smartFolders > 0) {
      out.push({
        id: SMART_FOLDERS_SECTION_ID,
        kind,
        accountId: null,
        colorSlot: null,
        collapsed: isCollapsed(SMART_FOLDERS_SECTION_ID),
      });
    }
  }
  return out;
}

/**
 * Reads a space-separated word option (`sections=`, `favorites=`, `chips=`):
 * the known words in the given order; `fallback` when the option is absent.
 */
export function wordsOption<T extends string>(
  value: unknown,
  known: readonly T[],
  fallback: readonly T[],
): T[] {
  if (typeof value !== "string") return [...fallback];
  const out: T[] = [];
  for (const word of value.split(/\s+/)) {
    if ((known as readonly string[]).includes(word) && !out.includes(word as T)) {
      out.push(word as T);
    }
  }
  return out;
}
