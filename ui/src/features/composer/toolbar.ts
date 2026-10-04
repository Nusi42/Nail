/**
 * The HTML editor's toolbar commands (ids name the Fluent messages
 * `composer-format-<id>` and `composer-format-<id>-short`).
 */
import type { Editor } from "@tiptap/core";

export type Chain = ReturnType<Editor["chain"]>;

export interface Command {
  id: string;
  run: (chain: Chain) => Chain;
  /** Mark or node name (and attributes) that shows the button as pressed. */
  active?: [string, Record<string, unknown>?];
}

/** The formatting commands of the toolbar, in order. */
export const FORMAT_COMMANDS: readonly Command[] = [
  { id: "bold", run: (c) => c.toggleBold(), active: ["bold"] },
  { id: "italic", run: (c) => c.toggleItalic(), active: ["italic"] },
  { id: "underline", run: (c) => c.toggleUnderline(), active: ["underline"] },
  { id: "strike", run: (c) => c.toggleStrike(), active: ["strike"] },
  { id: "heading", run: (c) => c.toggleHeading({ level: 2 }), active: ["heading", { level: 2 }] },
  { id: "bullet-list", run: (c) => c.toggleBulletList(), active: ["bulletList"] },
  { id: "ordered-list", run: (c) => c.toggleOrderedList(), active: ["orderedList"] },
  { id: "blockquote", run: (c) => c.toggleBlockquote(), active: ["blockquote"] },
  { id: "code-block", run: (c) => c.toggleCodeBlock(), active: ["codeBlock"] },
  {
    id: "table",
    run: (c) => c.insertTable({ rows: 3, cols: 3, withHeaderRow: true }),
  },
  { id: "undo", run: (c) => c.undo() },
  { id: "redo", run: (c) => c.redo() },
];

/** Commands shown while the cursor is in a table. */
export const TABLE_COMMANDS: readonly Command[] = [
  { id: "add-row", run: (c) => c.addRowAfter() },
  { id: "add-column", run: (c) => c.addColumnAfter() },
  { id: "delete-row", run: (c) => c.deleteRow() },
  { id: "delete-column", run: (c) => c.deleteColumn() },
  { id: "delete-table", run: (c) => c.deleteTable() },
];

/** Every toolbar command id (for the catalog coverage test). */
export const TOOLBAR_IDS = [
  ...FORMAT_COMMANDS.map((c) => c.id),
  ...TABLE_COMMANDS.map((c) => c.id),
  "link",
  "image",
  "source",
];

/** The Lucide icon of each toolbar command (M5a). */
export const FORMAT_ICONS: Readonly<Record<string, string>> = {
  bold: "bold",
  italic: "italic",
  underline: "underline",
  strike: "strikethrough",
  heading: "heading",
  "bullet-list": "list",
  "ordered-list": "list-ordered",
  blockquote: "text-quote",
  "code-block": "code",
  table: "table",
  undo: "undo-2",
  redo: "redo-2",
  "add-row": "plus",
  "add-column": "plus",
  "delete-row": "minus",
  "delete-column": "minus",
  "delete-table": "trash",
  link: "link",
  image: "image-plus",
  source: "file-text",
};
