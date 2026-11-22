/**
 * A list of folders as buttons, indented by depth (`folder-picker.*`
 * parts): "Move folder to…" (ADR 0196) picks a folder from it. The move
 * window's listbox options (`./FolderPicker.tsx`) show the same row body
 * (`FolderRowLabel`).
 */
import type { Folder } from "../api/generated";
import { useT } from "../i18n/I18nProvider";
import { folderIcon, Icon } from "./Icon";

/**
 * One folder row's body (`folder-picker.button` with `data-role` and
 * `data-folder`, its icon and name): a button with `onPick`, else a plain
 * element inside a listbox option.
 */
export function FolderRowLabel({
  folder,
  disabled = false,
  onPick,
}: {
  folder: Folder;
  disabled?: boolean;
  onPick?: (folder: Folder) => void;
}) {
  const body = (
    <>
      <Icon name={folderIcon(folder.role)} part="folder-picker.button.icon" />
      <span data-part="folder-picker.button.name">{folder.name}</span>
    </>
  );
  if (onPick === undefined) {
    return (
      <span data-part="folder-picker.button" data-role={folder.role} data-folder={folder.id}>
        {body}
      </span>
    );
  }
  return (
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
      {body}
    </button>
  );
}

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
          <FolderRowLabel folder={folder} disabled={disabled} onPick={onPick} />
        </li>
      ))}
    </ul>
  );
}
