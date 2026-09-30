/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en/customization.ftl`; ids built at
 * runtime are listed by `runtimeIds`.
 */
import type { MergeChange, MergeConflictKind, MergeSection } from "../../api/generated";
import { KEY_CONTEXTS } from "./editorModel";
import { DIALOGS, MERGE_FILES } from "./names";

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = [
  "inspector-",
  "merge-dialog-",
  "merge-section-",
  "merge-conflict-",
  "merge-change-",
  "config-error-",
  "saved-search-",
  "customize-",
];

/**
 * Literals with those prefixes that are not message ids: the `toolbar-order`
 * layout patch (the global `toolbar-` prefix catches it).
 */
export const notMessages: readonly string[] = ["toolbar-order", "toolbar-editor"];

const SECTIONS: MergeSection[] = [
  "header",
  "tokens",
  "modes",
  "parts",
  "icons",
  "message",
  "raw-css",
  "windows",
  "actions",
  "keys",
  "toolbars",
  "menus",
];
const CONFLICTS: MergeConflictKind[] = [
  "duplicate-key",
  "unknown-action",
  "unknown-widget",
  "drops-plugin-items",
  "invalid",
];
const CHANGES: MergeChange[] = ["added", "removed", "changed", "unchanged"];

/** Message ids built at runtime (e.g. `window-<name>` of dialogs). */
export function runtimeIds(): string[] {
  return [
    ...SECTIONS.map((s) => `merge-section-${s}`),
    ...CONFLICTS.map((k) => `merge-conflict-${k}`),
    ...CHANGES.map((c) => `merge-change-${c}`),
    ...MERGE_FILES.map((f) => `merge-dialog-file-${f}`),
    ...Object.values(DIALOGS).map((name) => `window-${name}`),
    ...["separator", "spacer"].map((k) => `customize-toolbar-${k}`),
    ...KEY_CONTEXTS.map((c) => `customize-keys-context-${c}`),
    "customize-saved",
    "customize-reset-done",
    "customize-keys-added",
    "customize-keys-removed",
    "inspector-hint",
    "inspector-hint-pinned",
    "customize-open-unsupported",
    // Labels the layout refers to (`label="@customize-action-…"`).
    "customize-action-inspector",
    "customize-action-merge",
    "customize-action-errors",
  ];
}
