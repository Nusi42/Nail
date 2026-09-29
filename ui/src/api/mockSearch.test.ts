import { matches, parseQuery, type Searchable } from "./mockSearch";

const message: Searchable = {
  subject: "Quarterly report draft",
  from: "Grace Hopper grace@navy.example",
  to: "Ada Lovelace ada@example.org",
  text: "Numbers for the board",
  tags: ["work"],
  account: "acc-work",
  accountName: "Work",
  folder: "Inbox",
  date: Date.parse("2026-09-20T10:00:00Z"),
  size: 50_000,
  unread: true,
  flagged: false,
  answered: false,
  draft: false,
  hasAttachments: true,
};

function check(text: string): boolean {
  const { node, error } = parseQuery(text);
  expect(error).toBeNull();
  return node === null || matches(node, message);
}

describe("mock query language", () => {
  it("matches fields, flags, dates, sizes and free text", () => {
    expect(check("from:grace")).toBe(true);
    expect(check("from:alan")).toBe(false);
    expect(check('subject:"report draft"')).toBe(true);
    expect(check("is:unread has:attachment")).toBe(true);
    expect(check("is:flagged")).toBe(false);
    expect(check("tag:work account:work folder:inbox")).toBe(true);
    expect(check("before:2026-09-21 after:2026-09-19")).toBe(true);
    expect(check("size>10k size<1M")).toBe(true);
    expect(check("board")).toBe(true);
    expect(check('"for the board"')).toBe(true);
  });

  it("supports AND, OR, NOT, minus and parentheses", () => {
    expect(check("from:alan OR from:grace")).toBe(true);
    expect(check("from:grace AND is:flagged")).toBe(false);
    expect(check("NOT is:flagged")).toBe(true);
    expect(check("-is:unread")).toBe(false);
    expect(check("(from:alan OR subject:quarterly) is:unread")).toBe(true);
  });

  it.each([
    ['subject:"open', "unterminated-quote", 8, 13],
    ["(from:ada", "unbalanced-parenthesis", 0, 1],
    ["from:ada)", "unbalanced-parenthesis", 8, 9],
    ["colour:red", "unknown-field", 0, 6],
    ["is:purple", "invalid-value", 3, 9],
    ["before:yesterday", "invalid-value", 7, 16],
    ["size>big", "invalid-value", 5, 8],
    ["from:ada AND", "unexpected-end", 12, 12],
    ["from:", "unexpected-end", 5, 5],
    ["OR from:ada", "unexpected-token", 0, 2],
    ["()", "unexpected-token", 1, 2],
  ])("reports %s as %s at %i..%i", (text, kind, start, end) => {
    expect(parseQuery(text).error).toMatchObject({ kind, start, end });
  });

  it("refuses queries nested too deeply or too long, like the core", () => {
    const deep = `${"(".repeat(1000)}a${")".repeat(1000)}`;
    expect(parseQuery(deep).error).toMatchObject({ kind: "too-complex", start: 64, end: 65 });
    expect(parseQuery(`${"NOT ".repeat(200)}a`).error?.kind).toBe("too-complex");
    expect(parseQuery("a ".repeat(3000)).error?.kind).toBe("too-complex");
    expect(parseQuery(`${"(".repeat(64)}a${")".repeat(64)}`).error).toBeNull();
  });

  it("counts positions in UTF-16 code units", () => {
    // "é" and the emoji take 1 and 2 code units: the error starts after them.
    const text = "é😀 colour:x";
    expect(parseQuery(text).error).toMatchObject({ kind: "unknown-field", start: 4, end: 10 });
  });

  it("treats an empty query as valid", () => {
    expect(parseQuery("   ")).toEqual({ node: null, error: null });
  });
});
