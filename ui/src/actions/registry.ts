/**
 * The action registry. Toolbars, menus and key bindings refer to actions by
 * id; the layout's `actions` section adds labels, icons and aliases.
 *
 * M0 implements navigation, pane toggling and sync. Actions that need write
 * requests (reply, archive, flags, …) arrive with M1; until then they are
 * registered as unavailable, so buttons render disabled and keys do nothing.
 */
import type { QueryClient } from "@tanstack/react-query";
import type { NailClient } from "../api/client";
import type { JsonValue, ResolvedLayout } from "../api/generated";
import type { UiStore } from "../state/uiStore";

export interface ActionContext {
  client: NailClient;
  store: UiStore;
  queryClient: QueryClient;
  layout: ResolvedLayout;
}

export interface ActionHandler {
  /** Runs the action. */
  run(ctx: ActionContext, args: readonly JsonValue[]): void | Promise<void>;
  /** Whether the action can run now (default: always). */
  enabled?(ctx: ActionContext, args: readonly JsonValue[]): boolean;
}

function step(ctx: ActionContext, delta: number): void {
  const { messageOrder, selectedMessageId, selectMessage } = ctx.store.getState();
  if (messageOrder.length === 0) return;
  const index = selectedMessageId === null ? -1 : messageOrder.indexOf(selectedMessageId);
  const next =
    index < 0
      ? delta > 0
        ? 0
        : messageOrder.length - 1
      : Math.min(messageOrder.length - 1, Math.max(0, index + delta));
  const id = messageOrder[next];
  if (id !== undefined) selectMessage(id);
}

/** Built-in handlers by action id. */
export const builtinActions: Record<string, ActionHandler> = {
  "mail.next": {
    run: (ctx) => step(ctx, 1),
    enabled: (ctx) => ctx.store.getState().messageOrder.length > 0,
  },
  "mail.previous": {
    run: (ctx) => step(ctx, -1),
    enabled: (ctx) => ctx.store.getState().messageOrder.length > 0,
  },
  "view.toggle": {
    run: (ctx, args) => {
      const id = args[0];
      if (typeof id === "string") ctx.store.getState().toggleNode(id);
    },
    enabled: (_ctx, args) => typeof args[0] === "string",
  },
  "sync.now": {
    run: async (ctx) => {
      await ctx.client.request(
        { type: "SyncNow", account_id: ctx.store.getState().activeAccountId },
        "SyncStarted",
      );
    },
  },
};

const MAX_ALIAS_DEPTH = 8;

export interface ResolvedAction {
  handler: ActionHandler | undefined;
  args: readonly JsonValue[];
}

/** Follows `alias_of` chains in the layout to a handler. */
export function resolveAction(
  id: string,
  args: readonly JsonValue[],
  layout: ResolvedLayout,
  handlers: Record<string, ActionHandler> = builtinActions,
): ResolvedAction {
  let currentId = id;
  let currentArgs = args;
  for (let depth = 0; depth < MAX_ALIAS_DEPTH; depth++) {
    if (Object.hasOwn(handlers, currentId)) {
      return { handler: handlers[currentId], args: currentArgs };
    }
    const def = Object.hasOwn(layout.actions, currentId) ? layout.actions[currentId] : undefined;
    if (!def?.alias_of) break;
    currentId = def.alias_of;
    currentArgs = def.args.length > 0 ? def.args : currentArgs;
  }
  return { handler: undefined, args: currentArgs };
}

export interface ActionDispatcher {
  dispatch(id: string, args?: readonly JsonValue[]): Promise<void>;
  isEnabled(id: string, args?: readonly JsonValue[]): boolean;
}

export function createDispatcher(
  ctx: ActionContext,
  handlers: Record<string, ActionHandler> = builtinActions,
): ActionDispatcher {
  return {
    async dispatch(id, args = []) {
      const { handler, args: finalArgs } = resolveAction(id, args, ctx.layout, handlers);
      if (!handler) {
        if (import.meta.env.DEV) console.info(`nail: action "${id}" is not available yet`);
        return;
      }
      if (handler.enabled && !handler.enabled(ctx, finalArgs)) return;
      try {
        await handler.run(ctx, finalArgs);
      } catch (error) {
        console.error(`nail: action "${id}" failed`, error);
      }
    },
    isEnabled(id, args = []) {
      const { handler, args: finalArgs } = resolveAction(id, args, ctx.layout, handlers);
      return handler !== undefined && (handler.enabled?.(ctx, finalArgs) ?? true);
    },
  };
}
