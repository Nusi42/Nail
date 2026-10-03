/**
 * The folder pane's behaviour outside React (M5b WP162): collapsing a
 * section (remembered in the core's UI state, `folder-pane.collapsed`) and
 * what choosing a favorite does. Used by the `folder-tree` widget and by the
 * `folders.toggle-section` action.
 */
import type { QueryClient } from "@tanstack/react-query";
import type { NailClient } from "../api/client";
import type { FavoriteFolder, Folder, FolderRole, JsonValue } from "../api/generated";
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
  /** Run an action (a window of its own). */
  | { kind: "action"; action: string };

/** The target of each favorite (`nail_api::views::FavoriteFolder`). */
export const FAVORITE_TARGETS: Readonly<Record<FavoriteFolder, FavoriteTarget>> = {
  "all-inboxes": { kind: "folder", role: "inbox" },
  unread: { kind: "search", text: "is:unread" },
  flagged: { kind: "search", text: "is:flagged" },
  snoozed: { kind: "action", action: "snooze.list" },
  drafts: { kind: "folder", role: "drafts" },
  sent: { kind: "folder", role: "sent" },
  outbox: { kind: "action", action: "outbox.open" },
};

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
