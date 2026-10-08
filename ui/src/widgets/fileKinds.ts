/**
 * The colour groups of attachment type badges (`data-kind`): each has a
 * theme colour `file-<kind>` (`defaults/theme.kdl`), which the skin uses
 * and the attachment card picks by file extension (Q2, SSOT).
 */
export const FILE_KINDS = [
  "pdf",
  "image",
  "document",
  "spreadsheet",
  "archive",
  "calendar",
  "other",
] as const;

/** The colour group of the type badge (`data-kind`). */
export type FileKind = (typeof FILE_KINDS)[number];
