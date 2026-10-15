/**
 * Names this feature registers (ADR 0194): the source window (title
 * `window-message-source` in `source.ftl`), the action and the root part.
 * Toolbars, menus and keys of `layout.kdl` refer to the action.
 */

/** The source of one message: `message-source:<n>`. */
export const MESSAGE_SOURCE_KIND = "message-source";

/** Action: shows the selected message's source. */
export const SHOW_SOURCE_ACTION = "source.message";

/** Root part of the window. */
export const SOURCE_PART = "message-source";

/** The window's views. */
export const SOURCE_VIEWS = ["original", "decoded"] as const;
export type SourceView = (typeof SOURCE_VIEWS)[number];
