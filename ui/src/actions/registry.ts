/**
 * The action registry. Toolbars, menus and key bindings refer to actions by
 * id; the layout's `actions` section adds labels, icons and aliases.
 *
 * Actions without a handler (for example plug-in actions that are not
 * installed) are unavailable: buttons render disabled and keys do nothing.
 * Writes (flags, move, delete) and sending go through the core, which queues
 * them (`state/mailOps.ts`, `state/compose.ts`).
 */
import type { QueryClient } from "@tanstack/react-query";
import type { NailClient } from "../api/client";
import type { FolderRole, JsonValue, MoveTarget, ResolvedLayout } from "../api/generated";
import type { MessageArgs } from "../i18n/localizer";
import {
  cancelRichSend,
  discardRich,
  saveRich,
  sendRich,
  startRichCompose,
  startRichReply,
} from "../features/composer/flows";
import { deleteMessages, findSummary, moveMessages, setFlags } from "../state/mailOps";
import type { UiStore } from "../state/uiStore";

export interface ActionContext {
  client: NailClient;
  store: UiStore;
  queryClient: QueryClient;
  layout: ResolvedLayout;
  /** Translation, for text actions put into messages (quote headers). */
  t: (id: string, args?: MessageArgs) => string;
  /** Locale for dates in generated text. */
  locale: string;
  /** Names of the dialogs features registered (`window.open` can open them). */
  dialogs?: ReadonlySet<string>;
}

/** Layout windows the built-in actions open. */
export const WINDOWS = {
  move: "move",
  accountSetup: "account-setup",
  composer: "composer",
} as const;

const ROLES: readonly FolderRole[] = ["inbox", "sent", "drafts", "archive", "junk", "trash"];

function selected(ctx: ActionContext): string[] {
  const id = ctx.store.getState().selectedMessageId;
  return id === null ? [] : [id];
}

const hasSelection = (ctx: ActionContext) => ctx.store.getState().selectedMessageId !== null;
const hasComposer = (ctx: ActionContext) => ctx.store.getState().composer !== null;

/** `mail.move-to "archive"` (a role) or `mail.move-to "<folder id>"`. */
export function moveTargetOf(arg: JsonValue | undefined): MoveTarget | null {
  if (typeof arg !== "string" || arg === "") return null;
  return (ROLES as readonly string[]).includes(arg)
    ? { role: arg as FolderRole }
    : { folder_id: arg };
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

  // ---- Message writes (queued by the core) ----
  "mail.archive": {
    run: (ctx) => moveMessages(ctx, selected(ctx), { role: "archive" }),
    enabled: hasSelection,
  },
  "mail.delete": {
    run: (ctx) => deleteMessages(ctx, selected(ctx), false),
    enabled: hasSelection,
  },
  "mail.delete-permanently": {
    run: (ctx) => deleteMessages(ctx, selected(ctx), true),
    enabled: hasSelection,
  },
  "mail.toggle-read": {
    run: (ctx) => {
      const ids = selected(ctx);
      const summary = ids[0] === undefined ? undefined : findSummary(ctx.queryClient, ids[0]);
      return setFlags(ctx, ids, { seen: !(summary?.flags.seen ?? false) });
    },
    enabled: hasSelection,
  },
  "mail.toggle-flag": {
    run: (ctx) => {
      const ids = selected(ctx);
      const summary = ids[0] === undefined ? undefined : findSummary(ctx.queryClient, ids[0]);
      return setFlags(ctx, ids, { flagged: !(summary?.flags.flagged ?? false) });
    },
    enabled: hasSelection,
  },
  "mail.move": {
    run: (ctx) => ctx.store.getState().openWindow(WINDOWS.move),
    enabled: hasSelection,
  },
  "mail.move-to": {
    run: (ctx, args) => {
      const target = moveTargetOf(args[0]);
      if (target) return moveMessages(ctx, selected(ctx), target);
    },
    enabled: (ctx, args) => hasSelection(ctx) && moveTargetOf(args[0]) !== null,
  },

  // ---- Composer ----
  // The rich composer (features/composer, ADR 0023): HTML / Markdown /
  // attachments; plain-text drafts behave as in M1a.
  "mail.compose": { run: (ctx) => startRichCompose(ctx) },
  "mail.reply": {
    run: (ctx) => startRichReply(ctx, selected(ctx)[0] ?? "", "reply"),
    enabled: hasSelection,
  },
  "mail.reply-all": {
    run: (ctx) => startRichReply(ctx, selected(ctx)[0] ?? "", "reply-all"),
    enabled: hasSelection,
  },
  "mail.forward": {
    run: (ctx) => startRichReply(ctx, selected(ctx)[0] ?? "", "forward"),
    enabled: hasSelection,
  },
  "composer.send": {
    run: async (ctx) => {
      await sendRich(ctx, null);
    },
    enabled: hasComposer,
  },
  "composer.save": {
    run: async (ctx) => {
      await saveRich(ctx);
    },
    enabled: hasComposer,
  },
  "composer.discard": { run: (ctx) => discardRich(ctx), enabled: hasComposer },
  "outbox.undo": {
    run: (ctx) => {
      const entry = ctx.store.getState().lastSend;
      if (entry) return cancelRichSend(ctx, entry.id);
    },
    enabled: (ctx) => ctx.store.getState().lastSend?.state === "scheduled",
  },

  // ---- Search, windows, accounts ----
  "search.focus": { run: (ctx) => ctx.store.getState().requestFocus("search") },
  "search.clear": { run: (ctx) => ctx.store.getState().setSearch(null) },
  "window.open": {
    run: (ctx, args) => {
      if (typeof args[0] === "string") ctx.store.getState().openWindow(args[0]);
    },
    enabled: (ctx, args) =>
      typeof args[0] === "string" &&
      (Object.hasOwn(ctx.layout.windows, args[0]) || (ctx.dialogs?.has(args[0]) ?? false)),
  },
  "window.close": {
    run: (ctx, args) =>
      ctx.store.getState().closeWindow(typeof args[0] === "string" ? args[0] : undefined),
    enabled: (ctx) => ctx.store.getState().windows.length > 0,
  },
  "account.add": {
    run: (ctx) => ctx.store.getState().openWindow(WINDOWS.accountSetup),
    enabled: (ctx) => Object.hasOwn(ctx.layout.windows, WINDOWS.accountSetup),
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
