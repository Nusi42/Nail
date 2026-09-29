/**
 * Rules feature aliases over the generated protocol: a dry-run match
 * carries its explanation (`RuleMatch.explanation`, one `ConditionHit` per
 * evaluated condition, produced by `nail_rules::ConditionHit`).
 */
import type { RuleMatch } from "../../api/generated";

export type { ConditionHit } from "../../api/generated";

/** A dry-run match with its explanation. */
export type ExplainedMatch = RuleMatch;
