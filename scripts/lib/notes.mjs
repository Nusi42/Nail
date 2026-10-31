/**
 * The project notes (`.claude/memory/*.md`) carry no session data and no
 * tool bookkeeping (ADR 0200). Claude Code writes three keys into the front
 * matter of the notes it saves, at the top level or indented under
 * `metadata`: `originSessionId` (ties the note to a private session),
 * `modified` (a time stamp) and `node_type`. The post-edit hook removes all
 * three (scripts/lib/after-edit.mjs); `scripts/check-notes.mjs` refuses the
 * session key (pre-commit, `npm run check`) and `--fix` removes all three.
 * Tests: scripts/test/notes.test.mjs.
 */

/** Where the notes live, relative to the checkout (`/` separators). */
export const NOTES = /^\.claude\/memory\/[^/]+\.md$/;

/** The front-matter key that names the session a note came from. */
export const SESSION_KEY = "originSessionId";

/** The front-matter keys Claude Code adds and the notes do without. */
export const TOOL_KEYS = [SESSION_KEY, "modified", "node_type"];

/** The leading `---` block of a note, its closing line included. */
const FRONT_MATTER = /^---\r?\n[\s\S]*?^---[ \t]*(?:\r?\n|$)/m;

/**
 * A front-matter line that sets one of `keys`, at any indentation.
 *
 * @param {readonly string[]} keys
 */
const keyLine = (keys) => new RegExp(`^[ \\t]*(?:${keys.join("|")})[ \\t]*:[^\\n]*(?:\\n|$)`, "gm");

/**
 * The front matter of `text` (empty when it has none).
 *
 * @param {string} text
 * @returns {string}
 */
function frontMatter(text) {
  const match = FRONT_MATTER.exec(text);
  return match?.index === 0 ? match[0] : "";
}

/**
 * Whether the front matter of `text` names a session.
 *
 * @param {string} text
 * @returns {boolean}
 */
export function hasSessionLine(text) {
  return keyLine([SESSION_KEY]).test(frontMatter(text));
}

/**
 * Whether the front matter of `text` sets any of {@link TOOL_KEYS}.
 *
 * @param {string} text
 * @returns {boolean}
 */
export function hasToolKeys(text) {
  return keyLine(TOOL_KEYS).test(frontMatter(text));
}

/**
 * `text` without the {@link TOOL_KEYS} lines of its front matter; the body
 * stays as it is.
 *
 * @param {string} text
 * @returns {string}
 */
export function withoutToolKeys(text) {
  const head = frontMatter(text);
  if (head === "") return text;
  return head.replace(keyLine(TOOL_KEYS), "") + text.slice(head.length);
}
