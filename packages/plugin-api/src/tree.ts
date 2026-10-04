/**
 * A quick check of component trees for plug-in authors and tests. The
 * host's `validate_tree` (Rust) is authoritative; this catches the common
 * mistakes early: too many or too deeply nested nodes, duplicate or missing
 * ids, images that are not raster `data:` URIs.
 */
import type { PluginUiNode } from "./generated";

/** Most nodes in one tree. */
export const MAX_UI_NODES = 500;
/** Deepest nesting (a top-level node has depth 1). */
export const MAX_UI_DEPTH = 8;
/** Most options, rows or messages of one node. */
export const MAX_UI_ITEMS = 500;
/** Image types an `image` node may carry. */
export const IMAGE_TYPES = ["image/png", "image/jpeg", "image/gif", "image/webp"] as const;

/** A problem found by {@link checkTree}. */
export interface TreeIssue {
  /** Where: `body[1].children[0]`. */
  path: string;
  /** What (English, for logs and test failures). */
  message: string;
}

const NEEDS_ID = new Set([
  "input",
  "textarea",
  "select",
  "checkbox",
  "date",
  "contact-picker",
  "folder-picker",
  "button",
]);

/** Checks a tree; returns every issue found (empty when it looks fine). */
export function checkTree(nodes: readonly PluginUiNode[], root = "body"): TreeIssue[] {
  const issues: TreeIssue[] = [];
  const ids = new Set<string>();
  let count = 0;
  const visit = (node: PluginUiNode, path: string, depth: number): void => {
    count += 1;
    if (count === MAX_UI_NODES + 1) {
      issues.push({ path, message: `more than ${String(MAX_UI_NODES)} nodes` });
    }
    if (depth > MAX_UI_DEPTH) {
      issues.push({ path, message: `nested deeper than ${String(MAX_UI_DEPTH)}` });
      return;
    }
    const id = "id" in node ? node.id : undefined;
    if (NEEDS_ID.has(node.type) && !id) {
      issues.push({ path, message: `<${node.type}> needs an id` });
    }
    if (
      (node.type === "message-list" || node.type === "table") &&
      node.selectable === true &&
      !id
    ) {
      issues.push({ path, message: `a selectable <${node.type}> needs an id` });
    }
    if (id) {
      if (ids.has(id)) issues.push({ path, message: `duplicate id "${id}"` });
      ids.add(id);
    }
    switch (node.type) {
      case "select":
        if (node.options.length > MAX_UI_ITEMS) {
          issues.push({ path, message: `more than ${String(MAX_UI_ITEMS)} options` });
        }
        for (const value of node.default ?? []) {
          if (!node.options.some((o) => o.value === value)) {
            issues.push({ path, message: `default "${value}" is not an option` });
          }
        }
        break;
      case "table":
        if (node.rows.length > MAX_UI_ITEMS) {
          issues.push({ path, message: `more than ${String(MAX_UI_ITEMS)} rows` });
        }
        node.rows.forEach((row, i) => {
          if (row.length !== node.columns.length) {
            issues.push({
              path: `${path}.rows[${String(i)}]`,
              message: "row length differs from columns",
            });
          }
        });
        break;
      case "message-list":
        if (node.messageIds.length > MAX_UI_ITEMS) {
          issues.push({ path, message: `more than ${String(MAX_UI_ITEMS)} messages` });
        }
        break;
      case "image": {
        const match = /^data:([a-z/]+);base64,/.exec(node.src);
        const type = match?.[1];
        if (!type || !(IMAGE_TYPES as readonly string[]).includes(type)) {
          if (/^[a-z][a-z0-9+.-]*:/i.test(node.src)) {
            issues.push({ path, message: "image src must be a PNG, JPEG, GIF or WebP data: URI" });
          }
          // Otherwise a package path (`assets/logo.png`), inlined by the host.
        }
        break;
      }
      case "progress":
        if (node.value !== undefined && (node.value < 0 || node.value > 100)) {
          issues.push({ path, message: "progress value must be 0–100" });
        }
        break;
      case "stack":
        node.children.forEach((child, i) => {
          visit(child, `${path}.children[${String(i)}]`, depth + 1);
        });
        break;
      default:
        break;
    }
  };
  nodes.forEach((node, i) => {
    visit(node, `${root}[${String(i)}]`, 1);
  });
  return issues;
}
