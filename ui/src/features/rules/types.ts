/**
 * Types of the rules feature that the generated protocol does not carry
 * (yet): the dry-run explanation. `nail-rules` produces it
 * (`nail_rules::ConditionHit`); the core adds it to each `RuleMatch` as
 * `explanation` once the contract has the field. Until then it is
 * optional and the preview simply shows no explanation.
 */
import type { RuleMatch } from "../../api/generated";

/** One evaluated condition of a dry run. */
export interface ConditionHit {
  /** Child indexes from the rule's root condition (empty = the root). */
  path: number[];
  /** Whether it matched. */
  matched: boolean;
  /** The message value that decided it, if any (header value, address …). */
  value: string | null;
}

/** A dry-run match with its explanation. */
export type ExplainedMatch = RuleMatch & { explanation?: ConditionHit[] };
