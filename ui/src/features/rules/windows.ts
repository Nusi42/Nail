/**
 * The rules feature's windows: the rules window itself and the editor's
 * two questions (R6, ADR 0239: modal confirmation windows over the rules
 * window, `./Questions.tsx`).
 */

/** The window with the rules (also the dialog's name). */
export const RULES_WINDOW = "rules";

/** "Delete the rule …?" (a destructive confirmation). */
export const RULE_DELETE_DIALOG = "rule-delete";

/** "… moves or deletes mail. Run it?" (a destructive confirmation). */
export const RULE_RUN_DIALOG = "rule-run";
