/**
 * Pure helpers of the Gmail UI: the label tree (`/` nests), which labels
 * the list shows, the search queries behind label views and category
 * filters, and a mirror of `GmailLabelSettings::issues` for immediate
 * feedback in the editor (the core validates for real).
 */
import type {
  GmailCategory,
  GmailLabel,
  GmailLabelIssue,
  GmailLabelSettings,
} from "../../api/generated";

/** Longest label name (`nail_api::gmail::MAX_LABEL_NAME`). */
export const MAX_LABEL_NAME = 225;

/** Gmail's system label ids; user labels cannot take these names. */
export const SYSTEM_LABELS = [
  "INBOX",
  "SENT",
  "DRAFT",
  "SPAM",
  "TRASH",
  "UNREAD",
  "STARRED",
  "IMPORTANT",
  "CHAT",
  "CATEGORY_PERSONAL",
  "CATEGORY_SOCIAL",
  "CATEGORY_PROMOTIONS",
  "CATEGORY_UPDATES",
  "CATEGORY_FORUMS",
] as const;

/** Inbox categories in Gmail's tab order. */
export const CATEGORIES: readonly GmailCategory[] = [
  "primary",
  "social",
  "promotions",
  "updates",
  "forums",
];

/** The label id of a category (`GmailCategory::label`). */
export function categoryLabel(category: GmailCategory): string {
  return category === "primary" ? "CATEGORY_PERSONAL" : `CATEGORY_${category.toUpperCase()}`;
}

/** The problems of label settings, like `GmailLabelSettings::issues` plus duplicates. */
export function labelIssues(
  settings: GmailLabelSettings,
  others: readonly GmailLabel[] = [],
): GmailLabelIssue[] {
  const out: GmailLabelIssue[] = [];
  const name = settings.name.trim();
  if (name === "") {
    out.push("missing-name");
  } else {
    if ([...name].length > MAX_LABEL_NAME) out.push("name-too-long");
    const hasControl = [...name].some((c) => {
      const code = c.codePointAt(0) ?? 0;
      return code < 0x20 || (code >= 0x7f && code < 0xa0);
    });
    if (
      SYSTEM_LABELS.some((s) => s.toLowerCase() === name.toLowerCase()) ||
      hasControl ||
      name.startsWith("/") ||
      name.endsWith("/") ||
      name.includes("//")
    ) {
      out.push("invalid-name");
    }
    if (others.some((l) => l.name.toLowerCase() === name.toLowerCase())) {
      out.push("duplicate-name");
    }
  }
  const hex = /^#[0-9a-fA-F]{6}$/;
  if (settings.color && !(hex.test(settings.color.background) && hex.test(settings.color.text))) {
    out.push("invalid-color");
  }
  return out;
}

/** A node of the label tree. */
export interface LabelNode {
  /** Full name (`Work/Projects`). */
  path: string;
  /** Last level (`Projects`). */
  name: string;
  /** The label, `null` for a level that has no label of its own. */
  label: GmailLabel | null;
  children: LabelNode[];
}

/** User labels as a tree, sorted by name on every level. */
export function labelTree(labels: readonly GmailLabel[]): LabelNode[] {
  const root: LabelNode[] = [];
  const sorted = labels
    .filter((l) => l.kind === "user")
    .sort((a, b) => a.name.localeCompare(b.name));
  for (const label of sorted) {
    const parts = label.name.split("/").filter(Boolean);
    let level = root;
    let path = "";
    parts.forEach((part, index) => {
      path = path ? `${path}/${part}` : part;
      let node = level.find((n) => n.name === part);
      if (!node) {
        node = { path, name: part, label: null, children: [] };
        level.push(node);
      }
      if (index === parts.length - 1) node.label = label;
      level = node.children;
    });
  }
  const sort = (nodes: LabelNode[]) => {
    nodes.sort((a, b) => a.name.localeCompare(b.name));
    nodes.forEach((n) => sort(n.children));
  };
  sort(root);
  return root;
}

/** Whether the label list shows a label (`labelListVisibility`). */
export function listed(label: GmailLabel, showHidden: boolean): boolean {
  switch (label.list_visibility) {
    case "show":
      return true;
    case "show-if-unread":
      return showHidden || label.unread > 0;
    case "hide":
      return showHidden;
  }
}

/** Keeps nodes whose label is listed, or that have listed children. */
export function pruneTree(nodes: readonly LabelNode[], showHidden: boolean): LabelNode[] {
  return nodes.flatMap((node) => {
    const children = pruneTree(node.children, showHidden);
    const own = node.label !== null && listed(node.label, showHidden);
    return own || children.length > 0 ? [{ ...node, children }] : [];
  });
}

/** Quotes a value for the query language when it needs it. */
function quoted(value: string): string {
  return /[\s()"]/.test(value) ? `"${value.replaceAll('"', "")}"` : value;
}

/** The search behind a label view (user labels are Nail tags). */
export function labelQuery(tag: string): string {
  return `tag:${quoted(tag)}`;
}

/** The search behind a category filter (`category:<name>`, ADR 0065). */
export function categoryQuery(category: GmailCategory): string {
  return `category:${category}`;
}

/** The category a search text filters on, if it is exactly a category filter. */
export function categoryOfQuery(text: string | null | undefined): GmailCategory | null {
  const match = /^category:([a-z]+)$/i.exec(text?.trim() ?? "");
  const name = match?.[1]?.toLowerCase();
  return CATEGORIES.find((c) => c === name) ?? null;
}

/** The parent path of a label name (`Work/Projects` → `Work`), `null` at the top. */
export function parentOf(name: string): string | null {
  const at = name.lastIndexOf("/");
  return at > 0 ? name.slice(0, at) : null;
}

/** The last level of a label name. */
export function leafOf(name: string): string {
  const at = name.lastIndexOf("/");
  return at >= 0 ? name.slice(at + 1) : name;
}

/** A label and everything nested under it would move together (renaming). */
export function isWithin(name: string, ancestor: string): boolean {
  return name === ancestor || name.startsWith(`${ancestor}/`);
}
