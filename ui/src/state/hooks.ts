/**
 * Hooks that bundle what write and composer flows need, for widgets that
 * call them directly (buttons inside widgets rather than layout actions).
 */
import { useQueryClient } from "@tanstack/react-query";
import { useMemo } from "react";
import { useLocale, useT } from "../i18n/I18nProvider";
import type { ComposeContext } from "./compose";
import { useClient } from "./queries";
import { useUi, useUiStoreApi } from "./uiStore";

/** Client, UI store, query cache, translation and locale. */
export function useComposeContext(): ComposeContext {
  const client = useClient();
  const store = useUiStoreApi();
  const queryClient = useQueryClient();
  const t = useT();
  const locale = useLocale();
  return useMemo(
    () => ({ client, store, queryClient, t, locale }),
    [client, store, queryClient, t, locale],
  );
}

/**
 * What a widget of the selected account reads: the UI store, the active
 * account and folder, and the current search.
 */
export function useActiveMailView() {
  const store = useUiStoreApi();
  const accountId = useUi((s) => s.activeAccountId);
  const activeFolderId = useUi((s) => s.activeFolderId);
  const search = useUi((s) => s.search);
  return { store, accountId, activeFolderId, search };
}

/** The ids a UI context (MCP, plug-in panels) is built from. */
export function useUiContextIds() {
  const accountId = useUi((s) => s.activeAccountId);
  const folderId = useUi((s) => s.activeFolderId);
  const messageId = useUi((s) => s.selectedMessageId);
  const threadId = useUi((s) => s.selectedThreadId);
  const draftId = useUi((s) => s.composer?.draftId ?? null);
  return { accountId, folderId, messageId, threadId, draftId };
}
