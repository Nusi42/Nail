/**
 * The project notes (`.claude/memory/*.md`) carry no session data (ADR 0200):
 * Claude Code adds an `originSessionId:` line to the front matter of every
 * note it writes, which ties the note to a private session. The post-edit
 * hook removes it, and `scripts/check-notes.mjs` refuses it (pre-commit,
 * `npm run check`). Tests: scripts/test/notes.test.mjs.
 */

/** Where the notes live, relative to the checkout (`/` separators). */
export const NOTES = /^\.claude\/memory\/[^/]+\.md$/;

/** A front-matter line naming the session a note came from. */
const SESSION_LINE = /^[ \t]*originSessionId:[^\n]*(?:\n|$)/gm;

/**
 * Whether `text` names a session.
 *
 * @param {string} text
 * @returns {boolean}
 */
export function hasSessionLine(text) {
  SESSION_LINE.lastIndex = 0;
  return SESSION_LINE.test(text);
}

/**
 * `text` without its session lines.
 *
 * @param {string} text
 * @returns {string}
 */
export function withoutSessionLines(text) {
  return text.replace(SESSION_LINE, "");
}
