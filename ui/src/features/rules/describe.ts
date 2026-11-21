/**
 * How the rules window names things in its lower panel (R2 design review,
 * items 6 and 7): an action with its value ("Add tag: project"), what a run
 * acted on, and why an older run cannot be undone. Pure functions over a
 * localizer, so they are tested without rendering.
 */
import type { FolderRef, RuleAction, RuleRun, RuleRunTarget } from "../../api/generated";

/** Localizes a Fluent id with arguments (the components' `t`). */
export type Localize = (id: string, args?: Record<string, string | number>) => string;

/** A folder reference as the user wrote it: its path, or its special role. */
export function folderRefText(t: Localize, folder: FolderRef): string {
  if (folder.type === "path") return folder.path;
  // Rules name no folder by the role "other" (the editor never offers it).
  return folder.role === "other" ? folder.role : t(`rules-role-${folder.role}`);
}

/** The value an action acts with, `null` for actions without one. */
function actionValue(t: Localize, action: RuleAction): string | null {
  switch (action.type) {
    case "move":
    case "copy":
      return folderRefText(t, action.folder);
    case "tag":
    case "untag":
      return action.tag;
    case "forward":
    case "redirect":
      return action.to.join(", ");
    case "flag":
      return t(action.flagged ? "rules-set-flag-on" : "rules-set-flag-off");
    case "mark-read":
      return t(action.read ? "rules-set-read-on" : "rules-set-read-off");
    case "notify":
      return t(action.notify ? "rules-set-notify-on" : "rules-set-notify-off");
    case "delete":
      return action.permanent ? t("rules-delete-permanent") : null;
    case "reply":
    case "archive":
    case "stop":
      return null;
  }
}

/** Actions whose value says it all ("Add the flag", not "Flag: Add the flag"). */
const SELF_NAMED: ReadonlySet<RuleAction["type"]> = new Set(["flag", "mark-read", "notify"]);

/** An action with its value: "Add tag: project", "Add the flag", "Archive". */
export function actionText(t: Localize, action: RuleAction): string {
  const value = actionValue(t, action);
  if (value !== null && SELF_NAMED.has(action.type)) return value;
  const kind = t(`rules-do-${action.type}`);
  return value === null || value === "" ? kind : t("rules-do-value", { action: kind, value });
}

/** What a run acted on; `folderName` names a folder by its id (`null`: unknown). */
export function runTargetText(
  t: Localize,
  target: RuleRunTarget,
  folderName: (id: string) => string | null,
): string {
  switch (target.kind) {
    case "scope":
      return t("rules-run-target-scope");
    case "messages":
      return t("rules-run-target-messages", { count: target.count });
    case "folder":
      return t("rules-run-target-folder", {
        folder: folderName(target.folder_id) ?? target.folder_id,
      });
  }
}

/** Why a run cannot be undone (Fluent id), `null` when it can. */
export function whyNoUndo(run: RuleRun): string | null {
  if (run.undoable) return null;
  if (run.state === "undone") return "rules-run-why-undone";
  if (run.state === "running") return "rules-run-why-running";
  if (run.state === "failed") return "rules-run-why-failed";
  if (run.op_ids.length === 0) return "rules-run-why-nothing";
  return "rules-run-why-final";
}
