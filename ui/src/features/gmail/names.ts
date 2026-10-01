/** Dialog and widget names of the Gmail feature. */

/** Create or edit a label (name, parent, colour, visibility). */
export const LABEL_EDITOR_DIALOG = "gmail-label-editor";
/** Confirm deleting a label. */
export const LABEL_DELETE_DIALOG = "gmail-label-delete";
/** Apply labels to the selected message. */
export const APPLY_LABELS_DIALOG = "gmail-apply-labels";

export const DIALOGS = [LABEL_EDITOR_DIALOG, LABEL_DELETE_DIALOG, APPLY_LABELS_DIALOG] as const;

/** Layout widget kinds (registered in `nail-config/src/widgets/gmail.rs`). */
export const LABEL_LIST_WIDGET = "label-list";
export const CATEGORY_TABS_WIDGET = "category-tabs";
