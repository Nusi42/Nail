/**
 * What a plug-in call carries from this UI: the UI's random `client_id`
 * (dialogs and notifications raised while handling the call come back only
 * to this UI; ADR 0050, "Which UI") and the UI context.
 */
import { PLUGIN_COMMAND_SURFACE_VALUES } from "../../api/generated";
import type { NailClient } from "../../api/client";
import type { PluginCommandSurface, UiContext } from "../../api/generated";
import type { UiState } from "../../state/uiStore";

const ids = new WeakMap<NailClient, string>();

function randomId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  return `ui-${Math.random().toString(36).slice(2)}${Date.now().toString(36)}`;
}

/** This UI's id towards the core (one per client connection). */
export function clientIdOf(client: NailClient): string {
  let id = ids.get(client);
  if (id === undefined) {
    id = randomId();
    ids.set(client, id);
  }
  return id;
}

/** Whether a dialog or notification with `clientId` belongs to this UI. */
export function isForThisUi(client: NailClient, clientId: string | null): boolean {
  return clientId === null || clientId === clientIdOf(client);
}

/** The part of the UI state a plug-in call's context is made of. */
export type ContextSource = Pick<
  UiState,
  "activeAccountId" | "activeFolderId" | "selectedMessageId" | "selectedThreadId"
> & { composer: { draftId: UiContext["draft_id"] } | null };

/** The UI context of the UI state (like the MCP context reporter). */
export function uiContextOf(state: ContextSource): UiContext {
  const messageId = state.selectedMessageId;
  return {
    account_id: state.activeAccountId,
    folder_id: state.activeFolderId,
    message_id: messageId,
    thread_id: state.selectedThreadId,
    selected_message_ids: messageId ? [messageId] : [],
    selected_text: null,
    draft_id: state.composer?.draftId ?? null,
    draft_cursor: null,
  };
}

const SURFACES: readonly PluginCommandSurface[] = PLUGIN_COMMAND_SURFACE_VALUES;

/** The surface of a keyboard context (`list`, `reader`, `composer`; else `global`). */
export function surfaceOfKeyContext(context: string): PluginCommandSurface {
  return (SURFACES as readonly string[]).includes(context)
    ? (context as PluginCommandSurface)
    : "global";
}
