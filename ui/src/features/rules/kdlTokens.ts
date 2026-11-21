/**
 * A small KDL v2 tokenizer for the rule text view's syntax highlighting
 * (R2 design review, item 14). It only colours text — the core reads the
 * rule (`RuleFromText`) — so it is forgiving: whatever it does not
 * recognise is plain text, and the tokens always add up to the input.
 * No dependency: a highlighter library would be far larger than the few
 * kinds rule text has.
 */

/** What a token is; the theme colours each kind (`rules.text.token[kind=…]`). */
export type TokenKind =
  "comment" | "string" | "number" | "keyword" | "node" | "property" | "punctuation" | "plain";

export interface Token {
  kind: TokenKind;
  text: string;
}

/** Characters that end a bare identifier (KDL v2 non-identifier characters). */
const NOT_IDENT = /[\s{}()[\]=;"\\/#]/;

/** The tokens of `text`, in order; joined they give `text` back. */
export function kdlTokens(text: string): Token[] {
  const tokens: Token[] = [];
  const push = (kind: TokenKind, value: string) => {
    if (value === "") return;
    const last = tokens.at(-1);
    if (last?.kind === kind && kind === "plain") last.text += value;
    else tokens.push({ kind, text: value });
  };
  // A node name is the first identifier of a line or after `{` / `;`.
  let atNodeStart = true;
  let i = 0;
  while (i < text.length) {
    const rest = text.slice(i);
    const ch = text.charAt(i);
    if (rest.startsWith("//")) {
      const end = rest.indexOf("\n");
      const comment = end < 0 ? rest : rest.slice(0, end);
      push("comment", comment);
      i += comment.length;
      continue;
    }
    if (rest.startsWith("/*")) {
      const end = rest.indexOf("*/", 2);
      const comment = end < 0 ? rest : rest.slice(0, end + 2);
      push("comment", comment);
      i += comment.length;
      continue;
    }
    if (ch === '"') {
      let j = 1;
      while (j < rest.length && rest.charAt(j) !== '"') j += rest.charAt(j) === "\\" ? 2 : 1;
      const string = rest.slice(0, Math.min(j + 1, rest.length));
      push("string", string);
      i += string.length;
      atNodeStart = false;
      continue;
    }
    if (ch === "#") {
      const keyword = /^#[A-Za-z-]+/.exec(rest)?.[0] ?? "#";
      push("keyword", keyword);
      i += keyword.length;
      atNodeStart = false;
      continue;
    }
    if ("{};=".includes(ch)) {
      push("punctuation", ch);
      if (ch !== "=") atNodeStart = true;
      i += 1;
      continue;
    }
    if (ch === "\n") {
      push("plain", ch);
      atNodeStart = true;
      i += 1;
      continue;
    }
    if (/\s/.test(ch)) {
      const space = /^[^\S\n]+/.exec(rest)?.[0] ?? ch;
      push("plain", space);
      i += space.length;
      continue;
    }
    const number = /^[+-]?\d[\d_]*(\.\d[\d_]*)?([eE][+-]?\d+)?(?=[\s{};=)]|$)/.exec(rest)?.[0];
    if (number !== undefined) {
      push("number", number);
      i += number.length;
      atNodeStart = false;
      continue;
    }
    let j = 0;
    while (j < rest.length && !NOT_IDENT.test(rest.charAt(j))) j += 1;
    if (j === 0) {
      push("plain", ch);
      i += 1;
      continue;
    }
    const word = rest.slice(0, j);
    const kind: TokenKind = rest.charAt(j) === "=" ? "property" : atNodeStart ? "node" : "plain";
    push(kind, word);
    i += j;
    atNodeStart = false;
  }
  return tokens;
}

/** The line numbers (1-based) of `text`'s lines. */
export function lineNumbers(text: string): number[] {
  return Array.from({ length: text.split("\n").length }, (_, i) => i + 1);
}
