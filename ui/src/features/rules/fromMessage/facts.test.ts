/**
 * "Values from this mail" (R2, ADR 0222): which values of a message a
 * condition or action field is offered, and what picking one does to the
 * condition or action.
 */
import type { Account, Condition, Folder, MessageFacts, RuleAction } from "../../../api/generated";
import { must } from "../../../test/must";
import {
  MAX_ADDRESSES_PER_FIELD,
  MAX_SUBJECT_WORDS,
  applyToAction,
  applyToCondition,
  domainOf,
  subjectWords,
  valuesForAction,
  valuesForCondition,
  type MessageValue,
} from "./facts";

const FACTS: MessageFacts = {
  message_id: "m1",
  account_id: "acc-work",
  folder_id: "f-lists",
  subject: "[nail-users] Release notes for M1",
  from: [{ name: "Nail Users", email: "users@lists.example.org" }],
  sender: [],
  to: [
    { name: null, email: "ada@example.com" },
    { name: "Grace", email: "grace@example.net" },
  ],
  cc: [{ name: null, email: "ada@example.com" }],
  reply_to: [{ name: null, email: "list@lists.example.org" }],
  list_id: "nail-users.lists.example.org",
  header_names: ["From", "To", "Subject", "List-Id", "X-Mailer"],
};

const text = (value = "") => ({ op: "contains", value, case_sensitive: false }) as const;
const from: Condition = { type: "address", field: "from", matches: text() };

function pick(values: readonly MessageValue[], kind: string, source?: string): MessageValue {
  return must(values.find((v) => v.kind === kind && (source === undefined || v.source === source)));
}

describe("domainOf / subjectWords", () => {
  it("takes the domain after the last @", () => {
    expect(domainOf("ada@mail.example.org")).toBe("mail.example.org");
    expect(domainOf("no-domain@")).toBeNull();
    expect(domainOf("plain")).toBeNull();
  });

  it("offers each word once, without surrounding punctuation or short words", () => {
    expect(subjectWords("[nail-users] Re: Release notes for M1, release!")).toEqual([
      "nail-users",
      "Release",
      "notes",
      "for",
    ]);
    const many = Array.from({ length: 30 }, (_, i) => `word${String(i)}`).join(" ");
    expect(subjectWords(many)).toHaveLength(MAX_SUBJECT_WORDS);
  });
});

describe("valuesForCondition", () => {
  it("offers address, name and domain of every address field, the condition's own first", () => {
    const values = valuesForCondition(FACTS, from);
    expect(values.map((v) => v.source)).toEqual([
      "from",
      "from",
      "from",
      "reply-to",
      "reply-to",
      "to",
      "to",
      "to",
      "to",
      "to",
      "cc",
      "cc",
    ]);
    expect(pick(values, "address", "from")).toEqual({
      kind: "address",
      source: "from",
      value: "users@lists.example.org",
      label: "Nail Users <users@lists.example.org>",
      op: "equals",
    });
    expect(pick(values, "name", "from").value).toBe("Nail Users");
    expect(pick(values, "domain", "from")).toMatchObject({
      value: "@lists.example.org",
      label: "lists.example.org",
      op: "ends-with",
    });
    // The recipients' field lists To and Cc first.
    const recipients = valuesForCondition(FACTS, { ...from, field: "recipients" });
    expect(recipients[0]?.source).toBe("to");
  });

  it("lists each value of a field once and caps a crowd of recipients", () => {
    const crowd = Array.from({ length: 25 }, (_, i) => ({
      name: null,
      email: `p${String(i)}@example.com`,
    }));
    const values = valuesForCondition({ ...FACTS, to: crowd }, { ...from, field: "to" });
    const to = values.filter((v) => v.source === "to");
    expect(to.filter((v) => v.kind === "address")).toHaveLength(MAX_ADDRESSES_PER_FIELD);
    expect(to.filter((v) => v.kind === "domain")).toHaveLength(1);
  });

  it("offers the whole subject and its words", () => {
    const values = valuesForCondition(FACTS, { type: "subject", matches: text() });
    expect(values[0]).toMatchObject({ kind: "subject", value: FACTS.subject, op: "equals" });
    expect(pick(values, "subject-word")).toMatchObject({ value: "nail-users", op: "contains" });
    expect(
      valuesForCondition({ ...FACTS, subject: " " }, { type: "subject", matches: text() }),
    ).toEqual([]);
  });

  it("offers the mailing list, or nothing without one", () => {
    const list: Condition = { type: "list-id", matches: null };
    expect(valuesForCondition(FACTS, list)).toEqual([
      {
        kind: "mailing-list",
        source: "list-id",
        value: "nail-users.lists.example.org",
        label: "nail-users.lists.example.org",
        op: "contains",
      },
    ]);
    expect(valuesForCondition({ ...FACTS, list_id: null }, list)).toEqual([]);
  });

  it("offers a known header's values (contains) and every header name", () => {
    const header: Condition = { type: "header", name: "reply-to", matches: null };
    const values = valuesForCondition(FACTS, header);
    expect(values[0]).toMatchObject({ source: "reply-to", op: "contains" });
    expect(values.filter((v) => v.kind === "header-name").map((v) => v.value)).toEqual(
      FACTS.header_names,
    );
    const other = valuesForCondition(FACTS, { ...header, name: "X-Mailer" });
    expect(other.every((v) => v.kind === "header-name")).toBe(true);
    const subject = valuesForCondition(FACTS, { ...header, name: "Subject" });
    expect(subject[0]).toMatchObject({ kind: "subject", op: "contains" });
  });

  it("names the account and the folder from the context, and the tags", () => {
    const accounts = [{ id: "acc-work", name: "Work", email: "me@example.com" }] as Account[];
    const folders = [{ id: "f-lists", path: "INBOX/Lists" }] as Folder[];
    const context = { accounts, folders, tags: ["work", "$label1"] };
    const account: Condition = { type: "account", account_id: "" };
    expect(valuesForCondition(FACTS, account, context)).toEqual([
      { kind: "account", source: "account", value: "acc-work", label: "Work", op: "equals" },
    ]);
    // Without the accounts the id stands for the name.
    expect(valuesForCondition(FACTS, account)[0]?.label).toBe("acc-work");
    const folder: Condition = {
      type: "folder",
      folder: { type: "path", path: "", account_id: null },
    };
    expect(valuesForCondition(FACTS, folder, context).map((v) => v.value)).toEqual(["INBOX/Lists"]);
    expect(valuesForCondition(FACTS, folder)).toEqual([]);
    expect(
      valuesForCondition(FACTS, { type: "tag", tag: "" }, context).map((v) => v.value),
    ).toEqual(["work", "$label1"]);
  });

  it("offers nothing for groups and fields without message values", () => {
    for (const condition of [
      { type: "all", conditions: [] },
      { type: "body", matches: text() },
      { type: "size", cmp: "greater", bytes: 1 },
      { type: "flag", flag: "seen", set: true },
    ] as Condition[]) {
      expect(valuesForCondition(FACTS, condition)).toEqual([]);
    }
  });
});

describe("applyToCondition", () => {
  it("sets the value and the comparison, keeping the case setting", () => {
    const values = valuesForCondition(FACTS, from);
    const domain = applyToCondition(
      { ...from, matches: { ...text("x"), case_sensitive: true } },
      pick(values, "domain", "from"),
    );
    expect(domain).toEqual({
      type: "address",
      field: "from",
      matches: { op: "ends-with", value: "@lists.example.org", case_sensitive: true },
    });
  });

  it("switches the address field to the value's, unless the field covers it", () => {
    const values = valuesForCondition(FACTS, from);
    const cc = pick(values, "address", "cc");
    expect(applyToCondition(from, cc)).toMatchObject({ field: "cc" });
    const recipients: Condition = { ...from, field: "recipients" };
    expect(applyToCondition(recipients, cc)).toMatchObject({ field: "recipients" });
  });

  it("puts a header name into the name, a header value into the test", () => {
    const header: Condition = { type: "header", name: "From", matches: null };
    const values = valuesForCondition(FACTS, header);
    expect(applyToCondition(header, pick(values, "header-name"))).toEqual({
      ...header,
      name: "From",
    });
    expect(applyToCondition(header, pick(values, "address"))).toEqual({
      ...header,
      matches: { op: "contains", value: "users@lists.example.org", case_sensitive: false },
    });
  });

  it("sets an account, a folder path (keeping its account) and a tag", () => {
    const value = (v: string) =>
      ({ kind: "folder", source: "folder", value: v, label: v, op: "equals" }) as const;
    expect(applyToCondition({ type: "account", account_id: "" }, value("acc-work"))).toEqual({
      type: "account",
      account_id: "acc-work",
    });
    expect(
      applyToCondition(
        { type: "folder", folder: { type: "role", role: "inbox", account_id: "acc-1" } },
        value("INBOX/Lists"),
      ),
    ).toEqual({
      type: "folder",
      folder: { type: "path", path: "INBOX/Lists", account_id: "acc-1" },
    });
    expect(applyToCondition({ type: "tag", tag: "" }, value("work"))).toEqual({
      type: "tag",
      tag: "work",
    });
    const body: Condition = { type: "body", matches: text() };
    expect(applyToCondition(body, value("x"))).toBe(body);
  });
});

describe("actions", () => {
  const context = {
    folders: [{ id: "f-lists", path: "INBOX/Lists" }] as Folder[],
    tags: ["work"],
  };

  it("offers the mail's folder for move / copy and its tags for tag / untag", () => {
    const move: RuleAction = { type: "move", folder: { type: "path", path: "", account_id: null } };
    const [folder] = valuesForAction(FACTS, move, context);
    expect(applyToAction(move, must(folder))).toEqual({
      type: "move",
      folder: { type: "path", path: "INBOX/Lists", account_id: null },
    });
    const untag: RuleAction = { type: "untag", tag: "" };
    const [tag] = valuesForAction(FACTS, untag, context);
    expect(applyToAction(untag, must(tag))).toEqual({ type: "untag", tag: "work" });
    expect(valuesForAction(FACTS, { type: "copy", folder: move.folder }, context)).toHaveLength(1);
  });

  it("offers nothing for actions without such a field", () => {
    const archive: RuleAction = { type: "archive" };
    expect(valuesForAction(FACTS, archive, context)).toEqual([]);
    expect(
      applyToAction(
        archive,
        pick(valuesForAction(FACTS, { type: "tag", tag: "" }, context), "tag"),
      ),
    ).toBe(archive);
  });
});
