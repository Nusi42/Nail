/**
 * A list of folders as buttons, indented by depth (`folder-picker.*`
 * parts): the message move picker and "Move folder to…" (ADR 0196) pick a
 * folder from it.
 */
import type { Folder } from "../api/generated";
import { useT } from "../i18n/I18nProvider";
import { Icon } from "./Icon";

export function FolderList({
  rows,
  disabled = false,
  onPick,
}: {
  rows: readonly { folder: Folder; depth: number }[];
  disabled?: boolean;
  onPick: (folder: Folder) => void;
}) {
  const t = useT();
  return (
    <ul data-part="folder-picker.list" aria-label={t("common-folders")}>
      {rows.map(({ folder, depth }) => (
        <li key={folder.id} data-part="folder-picker.item" data-depth={depth}>
          <button
            type="button"
            data-part="folder-picker.button"
            data-role={folder.role}
            data-folder={folder.id}
            disabled={disabled}
            onClick={() => {
              onPick(folder);
            }}
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
  );
}
