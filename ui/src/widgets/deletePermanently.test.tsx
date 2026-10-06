import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ActionTargets } from "../actions/mailTargets";
import { MOCK_CONVERSATION_ID, addMockConversation } from "../api/mockConversations";
import { mailActionsStoreFor } from "../features/mail-actions/store";
import { violations } from "../test/axe";
import { createEnv, renderWithProviders, type Env } from "../test/harness";
import { must } from "../test/must";
import { WindowLayer } from "../layout/WindowLayer";
import {
  DELETE_PERMANENTLY_DIALOG,
  askDeletePermanently,
  pendingPermanentDelete,
} from "./DeletePermanently";

const INBOX = "acc-personal/Inbox";
const TRASH = "acc-personal/Trash";

/** Puts messages into Trash (only from there can they be deleted for good). */
function toTrash(env: Env, ids: readonly string[]): void {
  for (const id of ids) {
    must(env.transport.data.messages.find((m) => m.id === id)).folder_id = TRASH;
  }
}

const messages = (ids: readonly string[]): ActionTargets => ({
  targets: ids.map((id) => ({ kind: "message", message_id: id })),
  source: "list",
});

async function setup(targets: ActionTargets, prepare: (env: Env) => void = () => undefined) {
  const env = createEnv();
  prepare(env);
  await act(() => askDeletePermanently(env, targets));
  const view = renderWithProviders(<WindowLayer layout={env.transport.data.layout} />, env);
  return { env, view };
}

describe("Delete permanently? (M5c, P3)", () => {
  it("opens as a modal confirmation and deletes the messages it was opened for", async () => {
    const user = userEvent.setup();
    const ids = [`${INBOX}#1`, `${INBOX}#2`];
    const { env } = await setup(messages(ids), (e) => {
      toTrash(e, ids);
    });
    const dialog = await screen.findByRole("dialog", { name: "Delete permanently?" });
    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(within(dialog).getByText("Delete 2 messages permanently?")).toBeInTheDocument();
    // The safe choice has the focus.
    await waitFor(() =>
      expect(within(dialog).getByRole("button", { name: "Cancel" })).toHaveFocus(),
    );
    // A selection change behind the window does not change what is deleted.
    env.store.getState().selectMessage(`${INBOX}#7`);
    await user.click(within(dialog).getByRole("button", { name: "Delete permanently" }));
    await waitFor(() => {
      expect(env.transport.requests).toContainEqual({
        type: "ApplyMailAction",
        targets: messages(ids).targets,
        action: { type: "delete-permanently" },
      });
    });
    expect(env.store.getState().windows).toEqual([]);
  });

  it("names the conversation's message count", async () => {
    const user = userEvent.setup();
    const target: ActionTargets = {
      targets: [{ kind: "conversation", thread_id: MOCK_CONVERSATION_ID, folder_id: TRASH }],
      source: "list",
    };
    let added: { id: string }[] = [];
    const { env } = await setup(target, (e) => {
      added = addMockConversation(e.transport.data);
      // Three of its messages (not the draft) are in Trash.
      toTrash(
        e,
        added.slice(0, 3).map((m) => m.id),
      );
    });
    const dialog = await screen.findByRole("dialog", { name: "Delete permanently?" });
    expect(
      within(dialog).getByText("Delete this conversation's 3 messages permanently?"),
    ).toBeInTheDocument();
    // A fourth message reaches Trash while the question is open: it was not
    // counted, so it is not deleted.
    const late = must(env.transport.data.messages.find((m) => m.id === added[3]?.id));
    late.folder_id = TRASH;
    await user.click(within(dialog).getByRole("button", { name: "Delete permanently" }));
    await waitFor(() => {
      expect(env.transport.requests).toContainEqual({
        type: "ApplyMailAction",
        targets: added.slice(0, 3).map((m) => ({ kind: "message", message_id: m.id })),
        action: { type: "delete-permanently" },
      });
    });
    expect(env.transport.data.messages).toContain(late);
  });

  it("Cancel deletes nothing", async () => {
    const user = userEvent.setup();
    const { env } = await setup(messages([`${INBOX}#3`]), (e) => {
      toTrash(e, [`${INBOX}#3`]);
    });
    const dialog = await screen.findByRole("dialog", { name: "Delete permanently?" });
    expect(within(dialog).getByText("Delete this message permanently?")).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(env.store.getState().windows).toEqual([]);
    expect(pendingPermanentDelete(env.store)).toBeNull();
    expect(env.transport.requests.some((r) => r.type === "ApplyMailAction")).toBe(false);
  });

  it("does not ask for mail outside Trash / Junk: the toast says why", async () => {
    const env = createEnv();
    await act(() => askDeletePermanently(env, messages([`${INBOX}#5`])));
    expect(env.store.getState().windows).not.toContain(DELETE_PERMANENTLY_DIALOG);
    expect(mailActionsStoreFor(env.store).getState().toast).toMatchObject({
      kind: "error",
      message: { id: "mail-action-issue-not-in-trash" },
    });
  });

  it("asks nothing without a selection and passes axe", async () => {
    const env = createEnv();
    await act(() => askDeletePermanently(env, messages([])));
    expect(env.store.getState().windows).not.toContain(DELETE_PERMANENTLY_DIALOG);
    const { view } = await setup(messages([`${INBOX}#4`]), (e) => {
      toTrash(e, [`${INBOX}#4`]);
    });
    await screen.findByRole("dialog", { name: "Delete permanently?" });
    expect(await violations(view.container)).toEqual([]);
  });
});
