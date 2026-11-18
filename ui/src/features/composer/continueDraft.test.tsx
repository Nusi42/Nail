/**
 * Continuing a draft from a Drafts folder: opening its row (double-click,
 * Enter) and `mail.continue-draft` open it in the composer through
 * `EditDraftMessage` instead of the reader.
 */
import { fireEvent, waitFor } from "@testing-library/react";
import { onTestFinished } from "vitest";
import type { MessageSummary } from "../../api/generated";
import { listFolders } from "../../api/mockMail";
import { mockBody, mockFolderId } from "../../api/mockData";
import { builtinActions, createDispatcher } from "../../actions/registry";
import { conversationActions } from "../conversation-reader/actions";
import { isDraftMessage } from "../mail-actions/run";
import { queryKeys } from "../../state/queries";
import { createEnv, renderNode, stubElementSizes, widget, type Env } from "../../test/harness";
import { actionContext, cacheList, requestsOf } from "../../test/env";
import { must } from "../../test/must";
import { FIRST } from "../../test/mockIds";
import { activeComposer } from "./sessions";

const DRAFTS = mockFolderId("acc-personal", "Drafts");

/** Adds a draft "Unfinished plan" to the personal Drafts folder of the mock core. */
function addDraftMessage(env: Env, flagged = true): MessageSummary {
  const data = env.transport.data;
  const base = must(data.messages.find((m) => m.id === FIRST));
  const summary: MessageSummary = {
    ...base,
    id: `${DRAFTS}#1`,
    folder_id: DRAFTS,
    subject: "Unfinished plan",
    from: { name: "Ada Lovelace", email: "ada@example.org" },
    to: [{ name: "Grace", email: "grace@navy.example" }],
    flags: { seen: true, flagged: false, answered: false, draft: flagged },
    thread_id: "t-unfinished",
    tags: [],
  };
  data.messages.push(summary);
  data.bodies.set(summary.id, mockBody(summary, { text: "Half written" }));
  return summary;
}

function draftsEnv(): Env {
  return createEnv({ state: { activeAccountId: "acc-personal", activeFolderId: DRAFTS } });
}

/** Lists the Drafts folder with a draft and clicks its row; the row. */
async function chosenDraftRow(env: Env): Promise<HTMLElement> {
  addDraftMessage(env);
  onTestFinished(stubElementSizes());
  const { container } = renderNode(widget("message-list"), env);
  const row = await waitFor(() =>
    must(container.querySelector<HTMLElement>('[data-part="message-list.row"]')),
  );
  fireEvent.click(row);
  return row;
}

/** The subject of the composer that opened, once it has. */
async function openedSubject(env: Env): Promise<string> {
  return waitFor(() => {
    const id = must(activeComposer(env.store.getState().windows));
    return must(env.store.getState().composers[id]).subject;
  });
}

describe("continuing a draft", () => {
  it("opens a draft's row in the composer on a double-click", async () => {
    const env = draftsEnv();
    const row = await chosenDraftRow(env);
    fireEvent.doubleClick(row);
    expect(await openedSubject(env)).toBe("Unfinished plan");
    expect(requestsOf(env, "EditDraftMessage")).toEqual([
      { type: "EditDraftMessage", message_id: `${DRAFTS}#1` },
    ]);
    const draft = [...env.transport.data.drafts.values()][0];
    expect(draft).toMatchObject({ subject: "Unfinished plan", text: "Half written" });
  });

  it("opens it with Enter, and the same draft again the next time", async () => {
    const env = draftsEnv();
    const row = await chosenDraftRow(env);
    fireEvent.keyDown(must(row.closest<HTMLElement>('[role="listbox"]')), { key: "Enter" });
    expect(await openedSubject(env)).toBe("Unfinished plan");
    await createDispatcher(actionContext(env)).dispatch("mail.continue-draft");
    expect(requestsOf(env, "EditDraftMessage")).toHaveLength(2);
    expect(env.transport.data.drafts.size).toBe(1);
  });

  it("is offered only for drafts", async () => {
    const env = draftsEnv();
    const flagged = addDraftMessage(env);
    const ctx = actionContext(env);
    const { isEnabled } = createDispatcher(ctx);
    expect(isEnabled("mail.continue-draft")).toBe(false);
    // An ordinary message is read, not continued.
    const inbox = must(env.transport.data.messages.find((m) => m.id === FIRST));
    cacheList(env, [inbox]);
    env.store.getState().selectMessage(inbox.id);
    expect(isEnabled("mail.continue-draft")).toBe(false);
    await createDispatcher(ctx, { ...builtinActions, ...conversationActions }).dispatch(
      "conversation.open-in-reader",
    );
    expect(requestsOf(env, "EditDraftMessage")).toEqual([]);
    // Flagged `\Draft`.
    cacheList(env, [flagged]);
    env.store.getState().selectMessage(flagged.id);
    expect(isEnabled("mail.continue-draft")).toBe(true);
  });

  it("counts a message of a Drafts folder as a draft without the flag", () => {
    const env = draftsEnv();
    const unflagged = addDraftMessage(env, false);
    const ctx = actionContext(env);
    cacheList(env, [unflagged]);
    expect(isDraftMessage(ctx, unflagged.id)).toBe(false);
    env.queryClient.setQueryData(
      queryKeys.folders("acc-personal"),
      listFolders(env.transport.data, "acc-personal"),
    );
    expect(isDraftMessage(ctx, unflagged.id)).toBe(true);
    expect(isDraftMessage(ctx, null)).toBe(false);
  });
});
