/**
 * Names this feature registers (ADR 0196): the actions of the folder pane's
 * menus and the folder windows (titles `window-<name>` in `folders.ftl`).
 */

/** Action: a new folder below the menu's folder (on an account heading: at its top). */
export const FOLDER_NEW_ACTION = "folder.new";
/** Action: renames the menu's folder. */
export const FOLDER_RENAME_ACTION = "folder.rename";
/** Action: moves the menu's folder below another one. */
export const FOLDER_MOVE_ACTION = "folder.move";
/** Action: deletes the menu's folder (into Trash, or for good after asking). */
export const FOLDER_DELETE_ACTION = "folder.delete";

/** Window: the name of a new or renamed folder. */
export const FOLDER_NAME_DIALOG = "folder-name";
/** Window: where to move a folder. */
export const FOLDER_MOVE_DIALOG = "folder-move";
/** Confirmation: delete a folder for good. */
export const FOLDER_DELETE_DIALOG = "folder-delete";

/** Root part of the folder windows. */
export const FOLDER_DIALOG_PART = "folder-dialog";
