import type { Rule } from "../../api/generated";
import { sampleRule } from "./mock";
import { ruleFromText, ruleIdBase, ruleToText } from "./mockText";

/** The canonical form nail-config writes for the design example (tests/rules.rs). */
const DESIGN_EXAMPLE = `rule "Newsletters" trigger="arrival" scope="account:Everything" {
    when any {
        header "List-Id" exists=#true
        query "from:*@substack.com"
    }
    then {
        tag "newsletter"
        move "Reading"
        notify #false
    }
}
`;

const strip = (r: Rule): Rule => ({ ...r, source: null });

describe("mock rule text", () => {
  it("reads and writes the design example like nail-config", () => {
    const parsed = ruleFromText(DESIGN_EXAMPLE);
    expect(parsed.errors).toEqual([]);
    const rule = parsed.rule as Rule;
    expect(rule).toMatchObject({
      id: "newsletters",
      name: "Newsletters",
      triggers: [{ type: "arrival" }],
      scope: [{ type: "account", account_id: "Everything" }],
      conditions: {
        type: "any",
        conditions: [
          { type: "header", name: "List-Id", matches: null },
          { type: "query", query: "from:*@substack.com" },
        ],
      },
      actions: [
        { type: "tag", tag: "newsletter" },
        { type: "move", folder: { type: "path", path: "Reading", account_id: null } },
        { type: "notify", notify: false },
      ],
    });
    expect(ruleToText(rule)).toBe(DESIGN_EXAMPLE);
  });

  it("round-trips every condition and action kind", () => {
    const rule: Rule = {
      id: "custom-id",
      name: 'Quotes "and" more',
      enabled: false,
      triggers: [
        { type: "arrival" },
        { type: "schedule", interval_minutes: 30 },
        { type: "older-than", days: 10 },
      ],
      scope: [
        { type: "account", account_id: "work" },
        { type: "folder", folder: { type: "role", role: "inbox", account_id: "home" } },
      ],
      conditions: {
        type: "all",
        conditions: [
          { type: "not", condition: { type: "header", name: "X-Spam", matches: null } },
          {
            type: "address",
            field: "reply-to",
            matches: { op: "regex", value: "^no-?reply@", case_sensitive: true },
          },
          { type: "subject", matches: { op: "starts-with", value: "[x]", case_sensitive: false } },
          { type: "body", matches: { op: "contains", value: "a\nb", case_sensitive: false } },
          { type: "query", query: 'subject:"a b" OR is:unread' },
          {
            type: "attachment",
            name: { op: "ends-with", value: ".pdf", case_sensitive: false },
            mime: null,
          },
          { type: "attachment", name: null, mime: null },
          { type: "size", cmp: "greater", bytes: 5000 },
          { type: "date", cmp: "after", date: "2026-01-02" },
          { type: "age", cmp: "less", days: 3 },
          { type: "flag", flag: "seen", set: false },
          { type: "tag", tag: "work" },
          { type: "account", account_id: "work" },
          { type: "folder", folder: { type: "path", path: "Lists/Rust", account_id: null } },
          { type: "auth", method: "spf", result: "softfail" },
          { type: "list-id", matches: { op: "contains", value: "rust", case_sensitive: false } },
          {
            type: "any",
            conditions: [
              { type: "tag", tag: "a" },
              { type: "not", condition: { type: "all", conditions: [] } },
            ],
          },
        ],
      },
      actions: [
        { type: "move", folder: { type: "role", role: "archive", account_id: null } },
        { type: "copy", folder: { type: "path", path: "Backup", account_id: "home" } },
        { type: "delete", permanent: true },
        { type: "archive" },
        { type: "tag", tag: "x" },
        { type: "untag", tag: "y" },
        { type: "flag", flagged: false },
        { type: "mark-read", read: true },
        { type: "forward", to: ["a@x.example", "b@x.example"], identity_id: "me" },
        { type: "redirect", to: ["c@x.example"] },
        { type: "reply", template: "Thanks!\nBye", identity_id: null },
        { type: "notify", notify: true },
        { type: "stop" },
      ],
      source: null,
    };
    const text = ruleToText(rule);
    const back = ruleFromText(text);
    expect(back.errors).toEqual([]);
    expect(strip(back.rule as Rule)).toEqual(rule);
  });

  it("derives ids like nail-config", () => {
    expect(ruleIdBase("  Rust: users list! ")).toBe("rust-users-list");
    expect(ruleIdBase("äöü")).toBe("rule");
    expect(ruleToText(sampleRule())).not.toContain("id=");
  });

  it("reports errors with positions", () => {
    expect(ruleFromText("").errors[0]?.message).toMatch(/found none/);
    expect(ruleFromText('rule "a" {}\nrule "b" {}').errors[0]?.message).toMatch(/found 2/);
    const bad = ruleFromText('rule "a" {\n  when {\n    frobnicate "x"\n  }\n}');
    expect(bad.rule).toBeNull();
    expect(bad.errors).toEqual([{ line: 3, column: 5, message: "unknown condition `frobnicate`" }]);
    expect(ruleFromText('rule "a" {\n  then { explode }\n}').errors[0]).toMatchObject({
      line: 2,
      message: "unknown action `explode`",
    });
    expect(ruleFromText('rule "a" {').errors[0]?.message).toMatch(/syntax error/);
    expect(ruleFromText("rule {}").errors[0]?.message).toMatch(/needs a name/);
  });
});
