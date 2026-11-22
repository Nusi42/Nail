/**
 * The mock core's mail actions on conversations (P3, ADR 0164): targets
 * resolve per action like the core's, every action is one batch with one
 * undo.
 */
import { TransportClient } from "./client";
import { MockTransport } from "./mock";
import {
  addMockConversation,
  MOCK_CONVERSATION_ID,
  MOCK_CONVERSATION_SUBJECT,
} from "./mockConversations";
import { createMockData } from "./mockData";
import { explicitTargetCount, kindOf, reachOf } from "./mockMailActions";
import { listFolderOf } from "../actions/mailTargets";
import { listRowTarget, rowFolderOf } from "../actions/rowTargets";
import type { MessageTarget } from "./generated";
import { MAX_MAIL_ACTION_TARGETS } from "./generated/constants";
import { must } from "../test/must";
import { INBOX, SENT, TRASH } from "../test/mockIds";
import { describeVectors } from "../test/vectors";
import mailActionVectors from "../../../crates/nail-api/fixtures/ts-vectors/mail-actions.json";

const ARCHIVE = "acc-personal/Archive";

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

describeVectors("mail actions", mailActionVectors, {
  reachOf,
  kindOf,
  explicitTargetCount,
  listFolderOf,
  rowFolderOf,
  listRowTarget,
});

describe("whole lists in the mock core (R5, ADR 0235)", () => {
  const inboxList = (except: MessageTarget[] = []): Extract<MessageTarget, { kind: "list" }> => ({
    kind: "list",
    list: { source: "conversations", folder_id: INBOX },
    except,
    as_of: null,
  });

  async function resolution(client: TransportClient, targets: MessageTarget[]) {
    return (
      await client.request(
        { type: "ResolveMailTargets", targets, action: { type: "mark-read", read: true } },
        "MailTargets",
      )
    ).resolution;
  }

  it("counts every row of the list but the deselected ones, without ids", async () => {
    const { client } = setup();
    const threads = (
      await client.request(
        { type: "ListThreads", folder_id: INBOX, offset: 0, limit: 1000 },
        "Threads",
      )
    ).threads;
    const oneByOne = await resolution(
      client,
      threads.map((t) => ({ kind: "conversation", thread_id: t.id, folder_id: INBOX })),
    );
    const all = await resolution(client, [inboxList()]);
    expect(all.message_ids).toEqual([]);
    expect(all.messages).toBe(oneByOne.messages);
    expect(all.conversations).toBe(threads.length);
    expect(all.as_of).not.toBeNull();
    const butOne = await resolution(client, [inboxList([row])]);
    expect(butOne.conversations).toBe(threads.length - 1);
    expect(butOne.messages).toBe(all.messages - 6);
  });

  it("acts only on what the confirmation counted (as_of)", async () => {
    const { client, transport } = setup();
    const { as_of: asOf, messages } = await resolution(client, [inboxList()]);
    // Mail that arrives after the confirmation stays untouched.
    const newcomer = {
      ...must(transport.data.messages[0]),
      id: `${INBOX}#late`,
      thread_id: "t-late",
    };
    transport.data.messages.push(newcomer);
    const { result } = await client.request(
      {
        type: "ApplyMailAction",
        targets: [{ ...inboxList(), as_of: asOf }],
        action: { type: "archive" },
      },
      "MailActionQueued",
    );
    expect(result.message_ids).not.toContain(newcomer.id);
    expect(result.message_ids.length).toBeLessThanOrEqual(messages);
    expect(must(transport.data.messages.find((m) => m.id === newcomer.id)).folder_id).toBe(INBOX);
    expect(result.batch_id).not.toBeNull();
  });

  it("counts each conversation of a search list once, like the core", async () => {
    const { client, added } = setup();
    // Everywhere: the conversation's Inbox and Sent messages are hits of
    // one conversation.
    expect(new Set(added.map((m) => m.folder_id)).size).toBeGreaterThan(1);
    const text = `subject:"${MOCK_CONVERSATION_SUBJECT}"`;
    const counted = await resolution(client, [
      {
        kind: "list",
        list: {
          source: "search",
          query: { text, account_id: null, folder_id: null, offset: 0, limit: 1, sort: "date" },
          conversations: true,
          listed_folder_id: null,
        },
        except: [],
        as_of: null,
      },
    ]);
    expect(counted.conversations).toBe(1);
    expect(counted.messages).toBeGreaterThan(1);
    // Two rows of one conversation named one by one count once too.
    expect((await resolution(client, [row, row])).conversations).toBe(1);
  });

  it("lists a search's hits and refuses nested lists and too many rows", async () => {
    const { client } = setup();
    const search = await resolution(client, [
      {
        kind: "list",
        list: {
          source: "search",
          query: {
            text: "",
            account_id: null,
            folder_id: INBOX,
            offset: 0,
            limit: 1,
            sort: "date",
          },
          conversations: false,
          listed_folder_id: INBOX,
        },
        except: [],
        as_of: null,
      },
    ]);
    expect(search.conversations).toBe(0);
    expect(
      await client.call({
        type: "ResolveMailTargets",
        targets: [inboxList([inboxList()])],
        action: { type: "archive" },
      }),
    ).toMatchObject({ localized: { id: "mail-action-issue-nested-list" } });
    const many: MessageTarget[] = Array.from({ length: MAX_MAIL_ACTION_TARGETS + 1 }, () => row);
    expect(
      await client.call({
        type: "ResolveMailTargets",
        targets: [inboxList(many)],
        action: { type: "archive" },
      }),
    ).toMatchObject({ localized: { id: "mail-action-issue-too-many-targets" } });
  });
});
