/**
 * Console output fails tests (installed by `setup.ts`).
 *
 * Vitest hides console output when stdout is not a terminal, so warnings
 * such as React's "not wrapped in act(...)", `nail: missing message …` or
 * `nail: formatting …` went unnoticed in piped runs. Every `console.error`
 * and `console.warn` during a test is therefore recorded, and the test fails
 * unless it declared the output with [`expectConsole`] or [`allowConsole`].
 *
 * `NAIL_CONSOLE_STACK=1 npx vitest run …` adds the call stack to each
 * reported line, e.g. to find the state update behind an act(...) warning.
 */
import { format } from "node:util";

type Level = "error" | "warn";

interface Rule {
  pattern: RegExp;
  /** `expectConsole`: the test fails when nothing matched. */
  required: boolean;
  matched: boolean;
}

interface Entry {
  level: Level;
  text: string;
}

let rules: Rule[] = [];
let entries: Entry[] = [];
const withStacks = Boolean(process.env.NAIL_CONSOLE_STACK);
if (withStacks) Error.stackTraceLimit = 60;

function asPattern(pattern: RegExp | string): RegExp {
  return typeof pattern === "string"
    ? new RegExp(pattern.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"))
    : pattern;
}

/**
 * Declares console output the current test expects: each pattern must match
 * at least one `console.error` / `console.warn` line of the test, and
 * matching lines do not fail it. Call it in the test (or its `beforeEach`).
 */
export function expectConsole(...patterns: (RegExp | string)[]): void {
  for (const p of patterns) rules.push({ pattern: asPattern(p), required: true, matched: false });
}

/**
 * Lets the current test log lines matching `patterns` without requiring
 * them (output whose occurrence the test does not control).
 */
export function allowConsole(...patterns: (RegExp | string)[]): void {
  for (const p of patterns) rules.push({ pattern: asPattern(p), required: false, matched: false });
}

function record(level: Level) {
  return (...args: unknown[]) => {
    const stack = withStacks ? `\n${new Error().stack ?? ""}` : "";
    entries.push({ level, text: format(...args) + stack });
  };
}

/** Routes `console.error` / `console.warn` into the recorder (again). */
export function installConsoleRecorder(): void {
  console.error = record("error");
  console.warn = record("warn");
}

/**
 * Starts a test with no rules and nothing recorded; re-installs the recorder
 * in case a test replaced `console.error` / `console.warn` with a spy.
 */
export function resetConsole(): void {
  rules = [];
  entries = [];
  installConsoleRecorder();
}

/**
 * The problems recorded since the last call: lines no rule of the current
 * test allows and, with `checkRequired`, expected patterns that never
 * matched (empty = fine). Clears the recorded lines, keeps the rules.
 */
export function consoleProblems(checkRequired: boolean): string[] {
  const problems: string[] = [];
  for (const entry of entries) {
    const rule = rules.find((r) => r.pattern.test(entry.text));
    if (rule) rule.matched = true;
    else
      problems.push(
        `unexpected console.${entry.level}: ${entry.text.slice(0, withStacks ? 20_000 : 2_000)}`,
      );
  }
  if (checkRequired) {
    for (const rule of rules) {
      if (rule.required && !rule.matched) {
        problems.push(`expected console output matching ${String(rule.pattern)}, got none`);
      }
    }
  }
  entries = [];
  return problems;
}
