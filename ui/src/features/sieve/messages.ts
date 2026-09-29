/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en/sieve.ftl`; ids built at
 * runtime are listed by `runtimeIds`.
 */
import { RULE_PLACEMENTS, SIEVE_STATES, VACATION_ISSUES } from "./model";

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = ["sieve-", "vacation-"];

/** Literals with those prefixes that are not message ids (part names). */
export const notMessages: readonly string[] = [];

const MANAGESIEVE_ISSUES = ["host", "port", "script-name"];

/** Message ids built at runtime. */
export function runtimeIds(): string[] {
  return [
    ...SIEVE_STATES.map((s) => `sieve-state-${s}`),
    ...RULE_PLACEMENTS.map((p) => `sieve-placement-${p}`),
    ...VACATION_ISSUES.map((i) => `vacation-issue-${i}`),
    ...MANAGESIEVE_ISSUES.map((i) => `sieve-issue-${i}`),
  ];
}
