/**
 * The mock core's mail actions on conversations (P3, ADR 0164): targets
 * resolve per action like the core's, every action is one batch with one
 * undo.
 */
import { TransportClient } from "./client";
import { MockTransport } from "./mock";
import { addMockConversation, MOCK_CONVERSATION_ID } from "./mockConversations";
import { createMockData } from "./mockData";
import { kindOf, reachOf } from "./mockMailActions";
import type { MessageTarget } from "./generated";
import { must } from "../test/must";
import { describeVectors } from "../test/vectors";
import mailActionVectors from "../../../crates/nail-api/fixtures/ts-vectors/mail-actions.json";

const INBOX = "acc-personal/Inbox";
const SENT = "acc-personal/Sent";
const ARCHIVE = "acc-personal/Archive";
const TRASH = "acc-personal/Trash";

function setup() {
  const transport = new MockTransport({ data: createMockData(8) });
  const added = addMockConversation(transport.data);
  return { transport, client: new TransportClient(transport), added };
}

const row: MessageTarget = {
  kind: "conversation",
  thread_id: MOCK_CONVERSATION_ID,
  folder_id: INBOX,
};

describe("mail actions on conversations in the mock core", () => {
  it("mirrors the core's reach rules", () => {
    expect(reachOf({ type: "archive" })).toBe("listed");
    expect(reachOf({ type: "trash" })).toBe("thread");
    expect(reachOf({ type: "flag", flagged: true })).toBe("newest");
    expect(reachOf({ type: "flag", flagged: false })).toBe("whole");
    expect(reachOf({ type: "mark-read", read: true })).toBe("whole");
  });

  it("archives the Inbox messages, keeps the replies in Sent, and undoes as one", async () => {
    const { client, transport, added } = setup();
    const { result } = await client.request(
      { type: "ApplyMailAction", targets: [row], action: { type: "archive" } },
      "MailActionQueued",
    );
    expect(result.kind).toBe("archive");
    expect(result.conversations).toBe(1);
    expect(result.message_ids).toHaveLength(4);
    const byId = (id: string) => must(transport.data.messages.find((m) => m.id === id));
    for (const id of result.message_ids) expect(byId(id).folder_id).not.toBe(INBOX);
    expect(added.filter((m) => m.folder_id === SENT)).toHaveLength(2);

    await client.request({ type: "UndoMailAction", batch_id: must(result.batch_id) }, "Queued");
    for (const id of result.message_ids) expect(byId(id).folder_id).toBe(INBOX);
    expect(
      await client.call({ type: "UndoMailAction", batch_id: must(result.batch_id) }),
    ).toMatchObject({ code: "conflict", localized: { id: "mail-action-issue-undone-already" } });
  });

  it("moves the whole conversation to Trash with the user's replies, never the draft", async () => {
    const { client, added } = setup();
    const { resolution } = await client.request(
      { type: "ResolveMailTargets", targets: [row], action: { type: "trash" } },
      "MailTargets",
    );
    expect(resolution.message_ids).toHaveLength(6);
    expect(resolution.message_ids).not.toContain(must(added.at(-1)).id);
  });

  it("deletes the whole thread like Gmail, never expunges from Trash, restores replies to Sent", async () => {
    const { client, transport, added } = setup();
    const byId = (id: string) => must(transport.data.messages.find((m) => m.id === id));
    // One message was archived on its own.
    byId(must(added[0]).id).folder_id = ARCHIVE;
    const { result } = await client.request(
      { type: "ApplyMailAction", targets: [row], action: { type: "trash" } },
      "MailActionQueued",
    );
    expect(result.message_ids).toHaveLength(6);
    expect(result.message_ids).toContain(must(added[0]).id);
    for (const id of result.message_ids) expect(byId(id).folder_id).toBe(TRASH);
    // Delete in Trash leaves the Trash copies (only Delete permanently removes them).
    const inTrash = { ...row, folder_id: TRASH };
    expect(
      await client.call({ type: "ApplyMailAction", targets: [inTrash], action: { type: "trash" } }),
    ).toMatchObject({ localized: { id: "mail-action-issue-no-messages" } });
    expect(transport.data.messages.filter((m) => m.folder_id === TRASH)).toHaveLength(6);
    // Move to Inbox from Trash: the user's replies go back to Sent.
    await client.request(
      {
        type: "ApplyMailAction",
        targets: [inTrash],
        action: { type: "move", target: { role: "inbox" } },
      },
      "MailActionQueued",
    );
    const folders = added.slice(0, 6).map((m) => byId(m.id).folder_id);
    expect(folders.filter((f) => f === SENT)).toHaveLength(2);
    expect(folders.filter((f) => f === INBOX)).toHaveLength(4);
  });

  it("flags the newest message and unflags every one", async () => {
    const { client, transport } = setup();
    const { result } = await client.request(
      { type: "ApplyMailAction", targets: [row], action: { type: "flag", flagged: true } },
      "MailActionQueued",
    );
    expect(result.message_ids).toEqual([`${INBOX}#nightingale-6`]);
    const { result: off } = await client.request(
      { type: "ApplyMailAction", targets: [row], action: { type: "flag", flagged: false } },
      "MailActionQueued",
    );
    expect(off.message_ids).toHaveLength(6);
    expect(
      transport.data.messages.filter(
        (m) => m.thread_id === MOCK_CONVERSATION_ID && m.flags.flagged,
      ),
    ).toHaveLength(0);
  });

  it("refuses to delete permanently outside Trash and Junk", async () => {
    const { client } = setup();
    expect(
      await client.call({
        type: "ApplyMailAction",
        targets: [row],
        action: { type: "delete-permanently" },
      }),
    ).toMatchObject({
      code: "invalid_request",
      localized: { id: "mail-action-issue-not-in-trash" },
    });
    expect(
      await client.call({ type: "ApplyMailAction", targets: [], action: { type: "archive" } }),
    ).toMatchObject({ localized: { id: "mail-action-issue-no-targets" } });
  });
});

describeVectors("mail actions", mailActionVectors, { reachOf, kindOf });
