/** The mock core's shared `SaveDraft` checks (plain and rich drafts alike). */
import { describe, expect, it } from "vitest";
import type { Draft } from "./generated";
import { createMockData } from "./mockData";
import { draftSaveRefusal, editDraftMessage, identityOf } from "./mockDrafts";
import type { MockCore } from "./mockMail";

function core(): MockCore {
  let n = 0;
  return {
    data: createMockData(5),
    emit: () => undefined,
    later: (fn) => {
      fn();
    },
    nextId: (prefix) => `${prefix}-${String(++n)}`,
  };
}

const draft = (patch: Partial<Draft> = {}): Draft => ({
  id: null,
  identity_id: "ada@example.org",
  to: [],
  cc: [],
  bcc: [],
  subject: "Hello",
  mode: "plain",
  text: "",
  html: null,
  markdown: null,
  attachments: [],
  context: null,
  signature: "identity",
  markdown_part: false,
  ...patch,
});

describe("mock drafts", () => {
  it("finds identities across accounts", () => {
    const c = core();
    expect(identityOf(c, "ada@example.org")?.email).toBe("ada@example.org");
    expect(identityOf(c, "nobody")).toBeUndefined();
  });

  it("refuses unknown identities and unknown drafts, and accepts the rest", () => {
    const c = core();
    expect(draftSaveRefusal(c, draft())).toBeNull();
    expect(draftSaveRefusal(c, draft({ identity_id: "nobody" }))).toMatchObject({
      type: "Error",
      code: "invalid_request",
    });
    expect(draftSaveRefusal(c, draft({ id: "draft-404" }))).toMatchObject({
      type: "Error",
      code: "not_found",
    });
  });

  it("continues a message as a draft of its account, once", () => {
    const c = core();
    const message = c.data.messages.find((m) => m.account_id === "acc-work");
    if (!message) throw new Error("no work message");
    const first = editDraftMessage(c, message.id);
    expect(first).toMatchObject({ type: "DraftSaved" });
    const saved = first.type === "DraftSaved" ? c.data.drafts.get(first.draft_id) : undefined;
    // Not sent by an identity: the account's first one.
    expect(saved).toMatchObject({ identity_id: "ada@work.example", subject: message.subject });
    expect(editDraftMessage(c, message.id)).toEqual(first);
    expect(editDraftMessage(c, "nope")).toMatchObject({ type: "Error", code: "not_found" });
  });
});
