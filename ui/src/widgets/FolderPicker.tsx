import { useState, type FormEvent } from "react";
import { errorMessageIdOf } from "../api/client";
import type { Folder } from "../api/generated";
import { useLabel, useT } from "../i18n/I18nProvider";
import { type WidgetProps } from "../layout/widgetRegistry";
import { useComposeContext } from "../state/hooks";
import { moveMessages } from "../state/mailOps";
import { useFolders } from "../state/queries";
import { useUi } from "../state/uiStore";
import { flattenFolders } from "./FolderTree";
import { Icon } from "./Icon";

/** Name of the layout window that holds the move picker. */
export const MOVE_WINDOW = "move";

/** Folders whose name or path contains the filter text (case-insensitive). */
export function filterFolders(folders: readonly Folder[], filter: string): Folder[] {
  const needle = filter.trim().toLowerCase();
  if (needle === "") return [...folders];
  return folders.filter(
    (f) => f.name.toLowerCase().includes(needle) || f.path.toLowerCase().includes(needle),
  );
}

/**
 * "Move to folder": a filterable list of the active account's folders. Picking
 * one moves the selected message there and closes the window.
 */
export function FolderPicker({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const ctx = useComposeContext();
  const accountId = useUi((s) => s.activeAccountId);
  const selectedId = useUi((s) => s.selectedMessageId);
  const closeWindow = useUi((s) => s.closeWindow);
  const folders = useFolders(accountId);
  const [filter, setFilter] = useState("");
  const [error, setError] = useState<string | null>(null);

  const rows = flattenFolders(folders.data ?? []);
  const matching = new Set(filterFolders(folders.data ?? [], filter).map((f) => f.id));
  const visible = rows.filter((r) => matching.has(r.folder.id));

  const move = async (folder: Folder) => {
    if (selectedId === null) return;
    try {
      await moveMessages(ctx, [selectedId], { folder_id: folder.id });
      closeWindow(MOVE_WINDOW);
    } catch (e) {
      setError(errorMessageIdOf(e));
    }
  };

  const submit = (event: FormEvent) => {
    event.preventDefault();
    const first = visible[0];
    if (first) void move(first.folder);
  };

  return (
    <form
      data-part="folder-picker"
      data-node-id={node.id ?? undefined}
      aria-label={node.label ? label(node.label) : t("folder-picker-label")}
      onSubmit={submit}
    >
      <label data-part="folder-picker.filter">
        <span data-part="folder-picker.filter.label">{t("folder-picker-filter")}</span>
        <input
          data-part="folder-picker.input"
          type="text"
          value={filter}
          autoComplete="off"
          onChange={(event) => setFilter(event.target.value)}
        />
      </label>
      {selectedId === null && (
        <div data-part="folder-picker.empty">{t("folder-picker-no-selection")}</div>
      )}
      {error && (
        <div data-part="folder-picker.error" role="alert">
          {t(error)}
        </div>
      )}
      {folders.isError && (
        <div data-part="folder-picker.error" role="alert">
          {t(errorMessageIdOf(folders.error))}
        </div>
      )}
      <ul data-part="folder-picker.list" aria-label={t("folder-picker-folders")}>
        {visible.map(({ folder, depth }) => (
          <li key={folder.id} data-part="folder-picker.item" data-depth={depth}>
            <button
              type="button"
              data-part="folder-picker.button"
              data-role={folder.role}
              data-folder={folder.id}
              disabled={selectedId === null}
              onClick={() => void move(folder)}
            >
              <Icon
                name={folder.role === "other" ? "folder" : folder.role}
                part="folder-picker.button.icon"
              />
              <span data-part="folder-picker.button.name">{folder.name}</span>
            </button>
          </li>
        ))}
      </ul>
      {folders.isSuccess && visible.length === 0 && (
        <div data-part="folder-picker.empty">{t("folder-picker-no-match")}</div>
      )}
    </form>
  );
}
