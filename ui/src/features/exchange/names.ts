/** Widget kinds, dialog names and action ids of the Exchange feature. */

/** Focused / Other tabs over an Exchange inbox (registered by WP90 in `nail-config`). */
export const FOCUSED_TABS_WIDGET = "focused-tabs";
/** A banner while automatic replies are on (registered by WP90). */
export const AUTOMATIC_REPLIES_BANNER_WIDGET = "automatic-replies-banner";
/** Shared and delegate mailboxes of the selected account (folder pane; needs registration). */
export const MAILBOXES_WIDGET = "exchange-mailboxes";
/** The category master list with colours (folder pane; needs registration). */
export const CATEGORIES_WIDGET = "exchange-categories";
/** Settings → Exchange inbox rules (needs registration). */
export const RULES_WIDGET = "exchange-rules";

export const WIDGETS = [
  FOCUSED_TABS_WIDGET,
  AUTOMATIC_REPLIES_BANNER_WIDGET,
  MAILBOXES_WIDGET,
  CATEGORIES_WIDGET,
  RULES_WIDGET,
] as const;

/** Add or edit another mailbox. */
export const MAILBOX_EDITOR_DIALOG = "exchange-mailbox-editor";
/** Confirm removing (or hiding) another mailbox. */
export const MAILBOX_REMOVE_DIALOG = "exchange-mailbox-remove";
/** Create or edit a category. */
export const CATEGORY_EDITOR_DIALOG = "exchange-category-editor";
/** Confirm deleting a category. */
export const CATEGORY_DELETE_DIALOG = "exchange-category-delete";
/** The automatic replies (out of office) editor. */
export const AUTOMATIC_REPLIES_DIALOG = "exchange-automatic-replies";
/** Move the selected message to Focused / Other (optionally always). */
export const CLASSIFY_DIALOG = "exchange-classify";
/** The categories of the selected message (WP95). */
export const APPLY_CATEGORIES_DIALOG = "exchange-apply-categories";

export const DIALOGS = [
  MAILBOX_EDITOR_DIALOG,
  MAILBOX_REMOVE_DIALOG,
  CATEGORY_EDITOR_DIALOG,
  CATEGORY_DELETE_DIALOG,
  AUTOMATIC_REPLIES_DIALOG,
  CLASSIFY_DIALOG,
  APPLY_CATEGORIES_DIALOG,
] as const;

/** Action ids (`exchange.*`). */
export const ACTIONS = [
  "exchange.add-mailbox",
  "exchange.new-category",
  "exchange.automatic-replies",
  "exchange.focus",
  "exchange.classify",
  "exchange.toggle-focused",
  "exchange.apply-categories",
] as const;
