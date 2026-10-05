/**
 * Gmail-like conversation actions through the whole app (P3, ADR 0164)
 * against the mock core: the keys and the reader act on the selected
 * conversation, the selection moves on, the undo toast undoes the batch, a
 * card's own menu acts on its message, picked rows act together.
 */
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "../../App";
import type { Request } from "../../api/generated";
import { MOCK_CONVERSATION_ID, addMockConversation } from "../../api/mockConversations";
import { violations } from "../../test/axe";
import { createEnv, setWindowWidth, stubElementSizes, type Env } from "../../test/harness";
import { must } from "../../test/must";
import { TOAST_MS, toastMessage } from "./Toast";
import { mailActionsStoreFor } from "./store";
import { removesRows } from "./run";

let restoreSizes: () => void;
beforeEach(() => {
  restoreSizes = stubElementSizes();
  setWindowWidth(1200);
});
afterEach(() => {
  restoreSizes();
  setWindowWidth(1024);
});

const TURNS = 100;

async function turn(): Promise<void> {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

async function eventually(assertion: () => void): Promise<void> {
  let failure: unknown = null;
  for (let n = 0; n < TURNS; n++) {
    await turn();
    try {
      assertion();
      return;
    } catch (error) {
      failure = error;
    }
  }
  throw failure;
}

async function settleUntil<T>(query: () => T | null, what: string): Promise<T> {
  for (let n = 0; n < TURNS; n++) {
    const found = query();
    if (found !== null) return found;
    await turn();
  }
  throw new Error(`${what} did not appear`);
}

function renderApp() {
  const env = createEnv({ state: { connection: "connecting" } });
  const added = addMockConversation(env.transport.data);
  render(<App client={env.client} queryClient={env.queryClient} store={env.store} />);
  return { env, added };
}

const conversationRow = () =>
  document.querySelector<HTMLElement>(
    `[data-part="message-list.row"][data-thread="${MOCK_CONVERSATION_ID}"]`,
  );

const actions = (env: Env) =>
  env.transport.requests.filter(
    (r): r is Extract<Request, { type: "ApplyMailAction" }> => r.type === "ApplyMailAction",
  );

const toast = () => document.querySelector<HTMLElement>('[data-part="mail-actions.toast"]');

describe("conversation actions (P3)", () => {
  it("archives the conversation with e, moves on, and undoes from the toast", async () => {
    const user = userEvent.setup();
    const { env, added } = renderApp();
    const row = await settleUntil(conversationRow, "the conversation's row");
    const rowIds = [...document.querySelectorAll<HTMLElement>('[data-part="message-list.row"]')];
    const below = rowIds[rowIds.indexOf(row) + 1]?.dataset.message ?? null;
    fireEvent.click(row);
    fireEvent.keyDown(document.body, { key: "e" });
    await eventually(() => {
      expect(actions(env).at(-1)).toEqual({
        type: "ApplyMailAction",
        targets: [
          {
            kind: "conversation",
            thread_id: MOCK_CONVERSATION_ID,
            folder_id: env.store.getState().activeFolderId,
          },
        ],
        action: { type: "archive" },
      });
    });
    // The Inbox messages went to Archive, the replies stay in Sent.
    const byFolder = (folder: string) =>
      added.filter((m) => m.folder_id.endsWith(`/${folder}`)).length;
    expect(byFolder("Archive")).toBe(4);
    expect(byFolder("Sent")).toBe(2);
    // The next conversation is selected, like Gmail.
    expect(env.store.getState().selectedMessageId).toBe(below);
    const shown = await settleUntil(toast, "the toast");
    expect(shown).toHaveTextContent("Conversation archived");
    expect(shown).toHaveAttribute("role", "status");
    expect(await violations(shown)).toEqual([]);

    await user.click(within(shown).getByRole("button", { name: "Undo" }));
    await eventually(() => {
      expect(env.transport.requests.some((r) => r.type === "UndoMailAction")).toBe(true);
      expect(byFolder("Archive")).toBe(0);
    });
    expect(byFolder("Inbox")).toBe(4);
    await eventually(() => {
      expect(toast()).toHaveTextContent("Action undone");
    });
    // The selection is back on the conversation.
    expect(env.store.getState().selectedThreadId).toBe(MOCK_CONVERSATION_ID);
  });

  it("deletes the whole conversation with #, Sent replies included; z undoes it", async () => {
    const { env, added } = renderApp();
    fireEvent.click(await settleUntil(conversationRow, "the conversation's row"));
    fireEvent.keyDown(document.body, { key: "#" });
    await eventually(() => {
      expect(actions(env).at(-1)?.action).toEqual({ type: "trash" });
      expect(toast()).toHaveTextContent("Conversation moved to Trash");
    });
    // Every message the row counts, never the draft.
    expect(added.filter((m) => m.folder_id.endsWith("/Trash"))).toHaveLength(6);
    expect(added.filter((m) => m.folder_id.endsWith("/Drafts"))).toHaveLength(1);
    fireEvent.keyDown(document.body, { key: "z" });
    await eventually(() => {
      expect(added.filter((m) => m.folder_id.endsWith("/Trash"))).toHaveLength(0);
    });
    expect(mailActionsStoreFor(env.store).getState().last).toBeNull();
  });

  it("acts on several picked conversations at once", async () => {
    const { env } = renderApp();
    const row = await settleUntil(conversationRow, "the conversation's row");
    const rows = [...document.querySelectorAll<HTMLElement>('[data-part="message-list.row"]')];
    const other = must(rows.find((r) => r !== row));
    fireEvent.click(row);
    fireEvent.click(other, { ctrlKey: true });
    await eventually(() => {
      expect(row).toHaveAttribute("aria-selected", "true");
      expect(other).toHaveAttribute("aria-selected", "true");
    });
    fireEvent.keyDown(document.body, { key: "e" });
    await eventually(() => {
      expect(actions(env).at(-1)?.targets).toHaveLength(2);
      expect(toast()).toHaveTextContent("2 conversations archived");
    });
    await eventually(() => {
      expect(env.store.getState().pendingOps).toEqual([]);
    });
  });

  it("a card's More menu acts on that message only", async () => {
    const user = userEvent.setup();
    const { env, added } = renderApp();
    fireEvent.click(await settleUntil(conversationRow, "the conversation's row"));
    const feed = await settleUntil(() => screen.queryByRole("feed"), "the feed");
    const card = await settleUntil(
      () =>
        feed.querySelector<HTMLElement>('[data-part="thread-view.card"][data-state~="expanded"]'),
      "an open card",
    );
    const id = must(card.dataset.message);
    await user.click(within(card).getByRole("button", { name: "More actions for this message" }));
    const menu = await screen.findByRole("menu");
    // The global keys act on the whole conversation: the card's items show none, Reply keeps its.
    const deleteItem = within(menu).getByRole("menuitem", { name: "Delete this message" });
    expect(deleteItem).not.toHaveAttribute("aria-keyshortcuts");
    expect(within(menu).getByRole("menuitem", { name: /^Reply$/ })).toHaveAttribute(
      "aria-keyshortcuts",
    );
    await user.click(deleteItem);
    await eventually(() => {
      expect(actions(env).at(-1)).toEqual({
        type: "ApplyMailAction",
        targets: [{ kind: "message", message_id: id }],
        action: { type: "trash" },
      });
      expect(toast()).toHaveTextContent("Message moved to Trash");
    });
    expect(added.filter((m) => m.folder_id.endsWith("/Trash")).map((m) => m.id)).toEqual([id]);
    // The conversation stays listed and open.
    expect(conversationRow()).not.toBeNull();
  });
});

describe("toast and list rules", () => {
  it("words what was done per conversation or message", () => {
    expect(
      toastMessage({
        kind: "done",
        seq: 1,
        action: "trash",
        conversations: 3,
        messages: 12,
        batchId: "b",
      }),
    ).toEqual({ id: "mail-actions-done-trash", args: { conversations: 3, messages: 12 } });
    expect(TOAST_MS.done).toBeGreaterThan(TOAST_MS.undone);
  });

  it("knows which actions take rows out of the list", () => {
    expect(removesRows({ type: "archive" }, "inbox", false)).toBe(true);
    expect(removesRows({ type: "archive" }, "archive", false)).toBe(false);
    expect(removesRows({ type: "archive" }, null, true)).toBe(false);
    expect(removesRows({ type: "trash" }, null, true)).toBe(true);
    expect(removesRows({ type: "mark-read", read: true }, "inbox", false)).toBe(false);
    expect(removesRows({ type: "junk", junk: false }, "junk", false)).toBe(true);
    expect(removesRows({ type: "junk", junk: false }, "inbox", false)).toBe(false);
  });
});
