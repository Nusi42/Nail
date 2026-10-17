/**
 * Commit message rules (ADR 0200): Conventional Commits subjects in plain
 * English, no session trailers or links to private claude.ai pages, and none
 * of the internal process vocabulary that means nothing to a reader of the
 * public history. Checked by the commit-msg hook
 * (scripts/check-commit-msg.mjs); tests in
 * scripts/test/commit-message.test.mjs.
 */

/** The longest subject line, type and scope included. */
export const MAX_SUBJECT = 72;

/** Conventional Commits types. */
export const TYPES = [
  "feat",
  "fix",
  "perf",
  "refactor",
  "test",
  "docs",
  "build",
  "ci",
  "chore",
  "style",
  "revert",
];

const SUBJECT = new RegExp(`^(?:${TYPES.join("|")})(?:\\([a-z0-9-]+\\))?!?: \\S`);

/**
 * What a commit message must not contain, with the reason shown to the
 * writer. Matched case-insensitively against every line of the message.
 */
export const FORBIDDEN = [
  { re: /^claude-session:/im, why: "no session trailer lines" },
  { re: /claude\.ai\//i, why: "no links to claude.ai pages" },
  { re: /\bWIN\d+\b|\bnail\.win\d+/i, why: "no work-folder or branch logistics" },
  { re: /\bwork folders?\b/i, why: "no work-folder or branch logistics" },
  { re: /\bwaves?\b|\bwork packages?\b|\bWP\d+/i, why: "no internal process vocabulary" },
  { re: /\b(?:M[0-5][a-d]|P[1-3]|Q[12])\b/, why: "no internal milestone codes" },
  { re: /\b(?:SSOT|ODR|DRY)\b/, why: "describe the change instead of SSOT / ODR / DRY" },
  { re: /\bwork in progress\b|\bWIP\b/i, why: "commit finished work only" },
];

/**
 * The message as git records it: comment lines and everything below the
 * scissors line removed.
 *
 * @param {string} raw the commit message file
 * @returns {string}
 */
export function messageText(raw) {
  return (raw.split(/^# -+ >8 -+$/m)[0] ?? "")
    .split("\n")
    .map((line) => (line.startsWith("#") ? "" : line))
    .join("\n");
}

/**
 * The problems of a commit message; empty when it follows the rules.
 *
 * @param {string} text the message (comments already removed)
 * @returns {string[]}
 */
export function commitMessageProblems(text) {
  const subject = text.trimStart().split("\n")[0]?.trimEnd() ?? "";
  /** @type {string[]} */
  const problems = [];
  if (!subject.startsWith("Merge ") && !SUBJECT.test(subject)) {
    problems.push(
      `the subject must read "<type>(<scope>): <subject>" with a type of ${TYPES.join(", ")}`,
    );
  }
  if (subject.length > MAX_SUBJECT) {
    problems.push(
      `the subject is ${String(subject.length)} characters long (at most ${String(MAX_SUBJECT)})`,
    );
  }
  for (const rule of FORBIDDEN) {
    rule.re.lastIndex = 0;
    const match = rule.re.exec(text);
    if (match) problems.push(`"${match[0].trim()}": ${rule.why}`);
  }
  return problems;
}
