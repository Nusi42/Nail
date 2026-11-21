/**
 * Starter rules (R2 design review, item 11): templates the empty rules
 * window offers. Each starts a new, unsaved rule the user finishes; names
 * and the folder a template files into are Fluent strings
 * (`rules-starter-<id>-name`, `-hint`, `rules-starter-newsletters-folder`).
 */
import type { Rule } from "../../api/generated";
import { newRule, textMatch } from "./model";

/** The starter templates, in the order the empty state lists them. */
export const STARTER_IDS = ["newsletters", "from-person", "old-read"] as const;
export type StarterId = (typeof STARTER_IDS)[number];

/** Localizes a Fluent id (the editor's `t`). */
type Localize = (id: string) => string;

/** The new rule a starter template begins. */
export function starterRule(id: StarterId, t: Localize): Rule {
  const name = t(`rules-starter-${id}-name`);
  switch (id) {
    case "newsletters":
      return {
        ...newRule(),
        name,
        conditions: { type: "all", conditions: [{ type: "list-id", matches: null }] },
        actions: [
          {
            type: "move",
            folder: { type: "path", path: t("rules-starter-newsletters-folder"), account_id: null },
          },
        ],
      };
    case "from-person":
      return {
        ...newRule(),
        name,
        conditions: {
          type: "all",
          conditions: [{ type: "address", field: "from", matches: textMatch("", "equals") }],
        },
        actions: [{ type: "flag", flagged: true }],
      };
    case "old-read":
      return {
        ...newRule(),
        name,
        triggers: [{ type: "older-than", days: 30 }],
        conditions: { type: "all", conditions: [{ type: "flag", flag: "seen", set: true }] },
        actions: [{ type: "archive" }],
      };
  }
}
