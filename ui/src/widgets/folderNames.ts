/**
 * How the folder pane names and orders folders (M5b review): special-use
 * folders (Inbox, Drafts, Sent, Archive, Junk, Trash) show their localized
 * role name — the server's raw name ("INBOX", "[Gmail]/Spam") stays in the
 * tooltip — and come first, in that fixed order, before the other folders
 * (which keep the core's order).
 */
import type { Folder, FolderRole } from "../api/generated";

/** The special-use roles in the pane's order. */
export const ROLE_ORDER: readonly FolderRole[] = [
  "inbox",
  "drafts",
  "sent",
  "archive",
  "junk",
  "trash",
];

/** The Fluent id of a special-use folder's name, `null` for other folders. */
export function folderRoleLabelId(folder: Pick<Folder, "role">): string | null {
  return ROLE_ORDER.includes(folder.role) ? `folder-role-${folder.role}` : null;
}

/** Sort key: the role's position, other folders after every role. */
function rank(folder: Pick<Folder, "role">): number {
  const index = ROLE_ORDER.indexOf(folder.role);
  return index < 0 ? ROLE_ORDER.length : index;
}

/** Siblings with the special-use folders first (stable for the rest). */
export function rolesFirst<T extends Pick<Folder, "role">>(folders: readonly T[]): T[] {
  return folders
    .map((folder, index) => ({ folder, index }))
    .sort((a, b) => rank(a.folder) - rank(b.folder) || a.index - b.index)
    .map((x) => x.folder);
}
