import { useRef, type KeyboardEvent } from "react";
import { errorMessageIdOf } from "../api/client";
import type { Folder } from "../api/generated";
import { useLabel, useT } from "../i18n/I18nProvider";
import { optionBool, type WidgetProps } from "../layout/widgetRegistry";
import { useFolders } from "../state/queries";
import { useUi } from "../state/uiStore";
import { Icon } from "./Icon";

/** Folders in tree order (parents before children) with their depth. */
export function flattenFolders(folders: readonly Folder[]): { folder: Folder; depth: number }[] {
  const byParent = new Map<string | null, Folder[]>();
  const ids = new Set(folders.map((f) => f.id));
  for (const folder of folders) {
    const parent = folder.parent_id !== null && ids.has(folder.parent_id) ? folder.parent_id : null;
    const list = byParent.get(parent) ?? [];
    list.push(folder);
    byParent.set(parent, list);
  }
  const out: { folder: Folder; depth: number }[] = [];
  const visit = (parent: string | null, depth: number) => {
    for (const folder of byParent.get(parent) ?? []) {
      out.push({ folder, depth });
      if (depth < 32) visit(folder.id, depth + 1);
    }
  };
  visit(null, 0);
  return out;
}

/** The folders of the active account, with unread counts. */
export function FolderTree({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const accountId = useUi((s) => s.activeAccountId);
  const activeFolderId = useUi((s) => s.activeFolderId);
  const selectFolder = useUi((s) => s.selectFolder);
  const folders = useFolders(accountId);
  const showUnread = optionBool(node, "show-unread", true);
  const items = useRef(new Map<string, HTMLLIElement>());
  const rows = flattenFolders(folders.data ?? []);

  const onKeyDown = (event: KeyboardEvent<HTMLUListElement>) => {
    const index = rows.findIndex((r) => r.folder.id === activeFolderId);
    let next: number;
    if (event.key === "ArrowDown") next = Math.min(rows.length - 1, index + 1);
    else if (event.key === "ArrowUp") next = Math.max(0, index - 1);
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = rows.length - 1;
    else return;
    event.preventDefault();
    const target = rows[next];
    if (target) {
      selectFolder(target.folder.id);
      items.current.get(target.folder.id)?.focus();
    }
  };

  const focusable = activeFolderId ?? rows[0]?.folder.id;

  return (
    <nav
      data-part="folder-tree"
      data-key-context="folders"
      data-node-id={node.id ?? undefined}
      aria-label={node.label ? label(node.label) : t("folder-tree-label")}
    >
      {folders.isError && (
        <div data-part="folder-tree.error">{t(errorMessageIdOf(folders.error))}</div>
      )}
      <ul
        data-part="folder-tree.list"
        role="tree"
        aria-label={t("folder-tree-label")}
        onKeyDown={onKeyDown}
      >
        {rows.map(({ folder, depth }) => {
          const selected = folder.id === activeFolderId;
          return (
            <li
              key={folder.id}
              role="treeitem"
              aria-level={depth + 1}
              aria-selected={selected}
              tabIndex={folder.id === focusable ? 0 : -1}
              data-part="folder-tree.item"
              data-state={selected ? "selected" : undefined}
              data-role={folder.role}
              data-depth={depth}
              data-folder={folder.id}
              ref={(el) => {
                if (el) items.current.set(folder.id, el);
                else items.current.delete(folder.id);
              }}
              onClick={() => selectFolder(folder.id)}
            >
              <Icon
                name={folder.role === "other" ? "folder" : folder.role}
                part="folder-tree.item.icon"
              />
              <span data-part="folder-tree.item.name">{folder.name}</span>
              {showUnread && folder.unread > 0 && (
                <span
                  data-part="folder-tree.item.unread-count"
                  aria-label={t("folder-tree-unread", { count: folder.unread })}
                >
                  {folder.unread}
                </span>
              )}
            </li>
          );
        })}
      </ul>
    </nav>
  );
}
