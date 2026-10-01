/** Test helpers of the Exchange feature (imported by the tests only). */
import type { InfiniteData } from "@tanstack/react-query";
import type { ComponentType } from "react";
import type { MessageSummary, Request, Response } from "../../api/generated";
import type { ActionContext } from "../../actions/registry";
import type { DialogProps } from "../../layout/dialogRegistry";
import { queryKeys } from "../../state/queries";
import { createEnv, type Env } from "../../test/harness";
import { dialogs } from ".";
import { EXCHANGE_ACCOUNT, seedExchangeDemo } from "./mockSeed";

export function setup(account: string | null = EXCHANGE_ACCOUNT): Env {
  const env = createEnv();
  seedExchangeDemo(env.transport.data);
  env.transport.data.behaviour.workMs = 0;
  if (account !== null) env.store.getState().selectAccount(account);
  return env;
}

export function requests<T extends Request["type"]>(
  env: Env,
  type: T,
): Extract<Request, { type: T }>[] {
  return env.transport.requests.filter((r) => r.type === type) as Extract<Request, { type: T }>[];
}

export function dialog(name: string): ComponentType<DialogProps> {
  const found = dialogs[name];
  if (!found) throw new Error(`no dialog ${name}`);
  return found;
}

export function message(env: Env, subject: string): MessageSummary {
  const found = env.transport.data.messages.find((m) => m.subject === subject);
  if (!found) throw new Error(`no message ${subject}`);
  return found;
}

/** Puts messages into the list cache, as the message list would. */
export function cacheList(env: Env, summaries: MessageSummary[]): void {
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
          messages: summaries,
        },
      ],
      pageParams: [0],
    },
  );
}

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
