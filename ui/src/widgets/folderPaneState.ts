/**
 * The folder pane's behaviour outside React (M5b WP162): collapsing a
 * section (remembered in the core's UI state, `folder-pane.collapsed`) and
 * what choosing a favorite does. Used by the `folder-tree` widget and by the
 * `folders.toggle-section` action.
 */
import type { QueryClient } from "@tanstack/react-query";
import type { NailClient } from "../api/client";
import type { FavoriteFolder, Folder, FolderRole, JsonValue } from "../api/generated";
import { SNOOZED_WINDOW } from "../features/triage/Snooze";
import {
  UI_STATE_COLLAPSED_SECTIONS,
  applyUiStateChange,
  stringList,
  uiStateKey,
  type UiStateMap,
} from "../state/uiState";

/** The collapsed section ids after toggling `id`. */
export function toggledSections(collapsed: readonly string[], id: string): string[] {
  return collapsed.includes(id) ? collapsed.filter((s) => s !== id) : [...collapsed, id];
}

/**
 * Collapses or expands a section: the cached UI state changes at once, the
 * core remembers it (`SetUiState`). A core that cannot store it keeps the
 * change for this session.
 */
export async function toggleCollapsedSection(
  client: NailClient,
  queryClient: QueryClient,
  id: string,
): Promise<void> {
  const state = queryClient.getQueryData<UiStateMap>(uiStateKey) ?? {};
  const current = stringList(
    Object.hasOwn(state, UI_STATE_COLLAPSED_SECTIONS)
      ? (state[UI_STATE_COLLAPSED_SECTIONS] ?? null)
      : null,
  );
  const next = toggledSections(current, id);
  const value: JsonValue | null = next.length > 0 ? next : null;
  applyUiStateChange(queryClient, UI_STATE_COLLAPSED_SECTIONS, value);
  try {
    const answer = await client.request(
      { type: "SetUiState", key: UI_STATE_COLLAPSED_SECTIONS, value },
      "UiState",
    );
    queryClient.setQueryData(uiStateKey, answer.state);
  } catch {
    // Not remembered across restarts; the pane still shows the change.
  }
}

/** What choosing a favorite does. */
export type FavoriteTarget =
  /** Show this folder of "All accounts". */
  | { kind: "folder"; role: FolderRole }
  /** Search every account. */
  | { kind: "search"; text: string }
  /**
   * Run an action that opens a window of its own (`window`: its id); the
   * favorite is selected while that window is the active view (R8).
   */
  | { kind: "action"; action: string; window: string };

/** The window of the Outbox (`outbox.open` = `window.open "outbox"` in `layout.kdl`). */
export const OUTBOX_WINDOW = "outbox";

/**
 * What the Unread / Flagged favorites leave out: mail in Trash and Junk is
 * neither waiting nor important (R8; `folder:` matches a folder's role).
 */
export const FAVORITE_SEARCH_EXCLUDES = "NOT folder:trash NOT folder:junk";

/** The query of a favorite that searches every account for `term`. */
const everywhereBut = (term: string) => `${term} ${FAVORITE_SEARCH_EXCLUDES}`;

/** The target of each favorite (`nail_api::views::FavoriteFolder`). */
export const FAVORITE_TARGETS: Readonly<Record<FavoriteFolder, FavoriteTarget>> = {
  "all-inboxes": { kind: "folder", role: "inbox" },
  unread: { kind: "search", text: everywhereBut("is:unread") },
  flagged: { kind: "search", text: everywhereBut("is:flagged") },
  snoozed: { kind: "action", action: "snooze.list", window: SNOOZED_WINDOW },
  drafts: { kind: "folder", role: "drafts" },
  sent: { kind: "folder", role: "sent" },
  outbox: { kind: "action", action: "outbox.open", window: OUTBOX_WINDOW },
};

/**
 * The window a favorite stands for when it is the active view (the topmost
 * of `windows`, the open windows in z-order), else `null`.
 */
export function favoriteWindowView(windows: readonly string[]): string | null {
  const top = windows[windows.length - 1];
  if (top === undefined) return null;
  const isFavoriteWindow = Object.values(FAVORITE_TARGETS).some(
    (target) => target.kind === "action" && target.window === top,
  );
  return isFavoriteWindow ? top : null;
}

/** The Lucide icon of each favorite. */
export const FAVORITE_ICONS: Readonly<Record<FavoriteFolder, string>> = {
  "all-inboxes": "inbox",
  unread: "mail",
  flagged: "flag",
  snoozed: "clock",
  drafts: "file-pen-line",
  sent: "send",
  outbox: "send-horizontal",
};

/** The top-level "All accounts" folder with a role (merged roles). */
export function unifiedFolder(folders: readonly Folder[], role: FolderRole): Folder | null {
  return folders.find((f) => f.role === role && f.parent_id === null) ?? null;
}

/**
 * The count a favorite shows: unread mail for the inboxes, every draft for
 * Drafts, nothing for the others (`null`).
 */
export function favoriteCount(favorite: FavoriteFolder, folders: readonly Folder[]): number | null {
  if (favorite === "all-inboxes") return unifiedFolder(folders, "inbox")?.unread ?? null;
  if (favorite === "drafts") return unifiedFolder(folders, "drafts")?.total ?? null;
  return null;
}
