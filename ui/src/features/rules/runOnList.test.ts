/**
 * The mock core's `RunRules` with a list target (R8, ADR 0251): the rules
 * run on every message the list stands for, minus its exceptions, and the
 * run is recorded like a run on named messages.
 */
import { describe, expect, it } from "vitest";
import type { ListTarget } from "../../api/generated";
import { folderMessages } from "../../api/mockMail";
import { INBOX } from "../../test/mockIds";
import { must } from "../../test/must";
import { addTagAllRule, setup } from "./testing";

const INBOX_LIST: ListTarget = {
  list: { source: "messages", folder_id: INBOX },
  except: [],
  as_of: null,
};

describe("RunRules with a list target", () => {
  it("runs on every message of the list except the deselected rows", async () => {
    const env = setup();
    addTagAllRule(env);
    const inbox = must(folderMessages(env.transport.data, INBOX));
    expect(inbox.length).toBeGreaterThan(1);
    const left = must(inbox[0]);
    const response = await env.transport.send({
      type: "RunRules",
      rule_ids: ["tag-all"],
      message_ids: [],
      folder_id: null,
      list: { ...INBOX_LIST, except: [{ kind: "message", message_id: left.id }] },
    });
    if (response.type !== "RulesRan") throw new Error(JSON.stringify(response));
    const run = must(response.runs[0]);
    expect(run.target).toEqual({ kind: "messages", count: inbox.length - 1 });
    expect(run.matched).toBe(inbox.length - 1);
  });

  it("refuses a list together with messages or a folder", async () => {
    const env = setup();
    addTagAllRule(env);
    const response = await env.transport.send({
      type: "RunRules",
      rule_ids: ["tag-all"],
      message_ids: [],
      folder_id: INBOX,
      list: INBOX_LIST,
    });
    expect(response).toMatchObject({ type: "Error", code: "invalid_request" });
  });
});
