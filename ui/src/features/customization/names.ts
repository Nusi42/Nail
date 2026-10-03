/**
 * Names shared by the customization slot's modules (and its message list).
 */
import type { ConfigFile } from "../../api/generated";

/** Dialog names (window names the layout does not define). */
export const DIALOGS = { merge: "config-merge", errors: "config-errors" } as const;

/** Files the merge dialog handles. */
export const MERGE_FILES = ["theme", "layout"] as const satisfies readonly ConfigFile[];
