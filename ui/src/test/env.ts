/**
 * Helpers the tests share on top of the harness's `Env`: the requests the
 * mock core received, a feature's event handler wired to the env's client,
 * the action context of the env, and a mock transport for feature mocks.
 */
import type { ActionContext } from "../actions/registry";
import type { InfiniteData } from "@tanstack/react-query";
import { resetLocalTimeZone, setLocalTimeZone } from "@internationalized/date";
import { act, screen, waitFor } from "@testing-library/react";
import { simulateExternalEdit } from "../api/editHistoryMock";
import type { UserEvent } from "@testing-library/user-event";
import type { Event as NailEvent, MessageSummary, Request, Response } from "../api/generated";
import { queryKeys } from "../state/queries";
import { MockTransport } from "../api/mock";
import { MOCK_CONVERSATION_ID, addMockConversation } from "../api/mockConversations";
import type { FeatureEventHandler } from "../features/types";
import type { Env } from "./harness";
import { DAY_MS } from "../state/clock";

/** The requests of `type` the env's mock core received, in order. */
export function requestsOf<T extends Request["type"]>(env: Env, type: T) {
  return env.transport.requests.filter((r): r is Extract<Request, { type: T }> => r.type === type);
}

/** Calls a feature's event handler for every event the env's client gets; returns the unsubscribe. */
export function wireEvents(env: Env, handle: FeatureEventHandler): () => void {
  return env.client.subscribe((event) => {
    handle(event, { client: env.client, queryClient: env.queryClient, store: env.store });
  });
}

/** What an action handler gets, from the env (texts are their ids, English dates). */
export function actionContext(env: Env): ActionContext {
  return {
    client: env.client,
    store: env.store,
    queryClient: env.queryClient,
    layout: env.transport.data.layout,
    t: (id) => id,
    locale: "en",
  };
}

/**
 * A mock transport for a feature mock's own tests: the events it emits,
 * and `send`, which fails unless the answer has the expected type.
 */
export function mockSetup() {
  const transport = new MockTransport();
  const events: NailEvent[] = [];
  transport.onEvent((e) => events.push(e));
  const send = async <T extends Response["type"]>(request: Request, type: T) => {
    const answer = await transport.send(request);
    if (answer.type !== type) throw new Error(JSON.stringify(answer));
    return answer as Extract<Response, { type: T }>;
  };
  return { transport, events, send };
}

/** Snoozes `messageId` for a day through the mock core. */
export async function snoozeForADay(env: Env, messageId: string): Promise<void> {
  await env.client.request(
    {
      type: "SnoozeMessages",
      message_ids: [messageId],
      until: new Date(Date.now() + DAY_MS).toISOString(),
      wake_folder_id: null,
    },
    "Queued",
  );
}

/** Records the requests the env's mock receives from now on (what did the UI ask for). */
export function recordRequests(env: Env): Request[] {
  const seen: Request[] = [];
  const send = env.transport.send.bind(env.transport);
  env.transport.send = (request) => {
    seen.push(request);
    return send(request);
  };
  return seen;
}

/** The mock message with `subject` (a test failure when there is none). */
export function messageWithSubject(env: Env, subject: string): MessageSummary {
  const found = env.transport.data.messages.find((m) => m.subject === subject);
  if (!found) throw new Error(`no message ${subject}`);
  return found;
}

/** Puts messages (of one folder) into the list cache, as the message list would. */
export function cacheList(env: Env, summaries: readonly MessageSummary[]): void {
  const folder = summaries[0]?.folder_id ?? null;
  env.queryClient.setQueryData<InfiniteData<Extract<Response, { type: "Messages" }>>>(
    queryKeys.messages(folder, 50),
    {
      pages: [
        {
          type: "Messages",
          folder_id: folder ?? "",
          offset: 0,
          total: summaries.length,
          messages: [...summaries],
        },
      ],
      pageParams: [0],
    },
  );
}

/** Clicks the button `name` and waits until the dialog called `close`. */
export async function clickAndClose(
  user: UserEvent,
  name: string,
  close: () => void,
): Promise<void> {
  await user.click(screen.getByRole("button", { name }));
  await waitFor(() => {
    expect(close).toHaveBeenCalled();
  });
}

/** Runs every test of the file with the local time zone `zone`. */
export function inTimeZone(zone: string): void {
  beforeAll(() => {
    setLocalTimeZone(zone);
  });
  afterAll(() => {
    resetLocalTimeZone();
  });
}

/**
 * Changes `layout.kdl` on disk while the UI still holds an older revision
 * of its edit history: the next layout edit is a conflict.
 */
export function staleLayoutRevision(env: Env): void {
  act(() => {
    simulateExternalEdit(env.transport, "layout");
  });
  env.queryClient.setQueryData(["edit-history", "layout"], {
    scope: "layout",
    revision: "stale",
    undo: [],
    redo: [],
  });
}

/** Caches `summary` in its folder's list and selects it, as a click in the list would. */
export function selectCached(env: Env, summary: MessageSummary): void {
  cacheList(env, [summary]);
  env.store.getState().selectMessage(summary.id);
}

/** Replaces `window.print` around every test with a spy that reports "printed" at once. */
export function stubPrintEachTest(): { readonly spy: ReturnType<typeof vi.fn> } {
  const stub = { spy: vi.fn() };
  beforeEach(() => {
    stub.spy = vi.fn(() => window.dispatchEvent(new Event("afterprint")));
    window.print = stub.spy;
  });
  return stub;
}

/** Saves a plain message "Later" to Grace and queues it (`delay` / `sendAt`); the outbox entry. */
export async function queueLater(env: Env, delay: number | null, sendAt: string | null) {
  const { draft_id } = await env.client.request(
    {
      type: "SaveDraft",
      draft: {
        id: null,
        identity_id: "ada@example.org",
        to: [{ name: "Grace", email: "grace@navy.example" }],
        cc: [],
        bcc: [],
        subject: "Later",
        mode: "plain",
        text: "x",
        html: null,
        markdown: null,
        attachments: [],
        context: null,
        signature: "identity",
        markdown_part: false,
      },
    },
    "DraftSaved",
  );
  return (
    await env.client.request(
      { type: "SendDraft", draft_id, delay_secs: delay, send_at: sendAt },
      "SendQueued",
    )
  ).entry;
}

/**
 * Adds the mock conversation and selects its newest message (the draft
 * reply that never joins it aside); its messages, oldest first.
 */
export function selectMockConversation(env: Env): MessageSummary[] {
  const messages = addMockConversation(env.transport.data);
  const newest = messages.filter((m) => !m.flags.draft).at(-1);
  act(() => {
    if (newest) env.store.getState().selectMessage(newest.id);
    env.store.getState().selectThread(MOCK_CONVERSATION_ID);
  });
  return messages;
}

/** The declarations of an element's geometry rule in the compiled theme. */
export function geometry(element: Element): Record<string, string> {
  const id = element.getAttribute("data-geom") ?? "";
  const rule = [
    ...(document.head.querySelector<HTMLStyleElement>("style[data-nail-theme]")?.sheet?.cssRules ??
      []),
  ].find(
    (r): r is CSSStyleRule => r instanceof CSSStyleRule && r.selectorText === `[data-geom="${id}"]`,
  );
  const out: Record<string, string> = {};
  if (!rule) return out;
  for (let i = 0; i < rule.style.length; i++) {
    const property = rule.style.item(i);
    out[property] = rule.style.getPropertyValue(property);
  }
  return out;
}
