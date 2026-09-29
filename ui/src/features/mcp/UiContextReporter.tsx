/**
 * Background effect: reports the UI context to the core
 * (`ReportUiContext`) whenever the open account, folder, message, thread,
 * reader selection or composer draft changes, so MCP clients holding the
 * `context` grant can read `nail://ui/context`. Reports are debounced and
 * only sent when something changed. Renders nothing.
 */
import { useEffect, useRef, useState } from "react";
import type { UiContext } from "../../api/generated";
import { useClient } from "../../state/queries";
import { useUi } from "../../state/uiStore";

/** Wait after the last change before reporting (ms). */
export const REPORT_DELAY_MS = 250;
/** Longest selected text reported. */
export const MAX_SELECTED_TEXT = 4000;

/** Text the user selected inside the message reader, if any. */
export function readerSelection(selection: Selection | null): string | null {
  if (!selection || selection.isCollapsed || selection.rangeCount === 0) return null;
  const node = selection.anchorNode;
  const element = node instanceof Element ? node : (node?.parentElement ?? null);
  if (!element?.closest('[data-part^="message-view"]')) return null;
  const text = selection.toString().trim();
  return text === "" ? null : text.slice(0, MAX_SELECTED_TEXT);
}

export function UiContextReporter() {
  const client = useClient();
  const accountId = useUi((s) => s.activeAccountId);
  const folderId = useUi((s) => s.activeFolderId);
  const messageId = useUi((s) => s.selectedMessageId);
  const threadId = useUi((s) => s.selectedThreadId);
  const draftId = useUi((s) => s.composer?.draftId ?? null);
  const [selectedText, setSelectedText] = useState<string | null>(null);
  const last = useRef<string | null>(null);

  useEffect(() => {
    const onSelection = () => setSelectedText(readerSelection(document.getSelection()));
    document.addEventListener("selectionchange", onSelection);
    return () => document.removeEventListener("selectionchange", onSelection);
  }, []);

  useEffect(() => {
    const context: UiContext = {
      account_id: accountId,
      folder_id: folderId,
      message_id: messageId,
      thread_id: threadId,
      selected_message_ids: messageId ? [messageId] : [],
      selected_text: messageId ? selectedText : null,
      draft_id: draftId,
      draft_cursor: null,
    };
    const key = JSON.stringify(context);
    if (key === last.current) return;
    const timer = setTimeout(() => {
      last.current = key;
      // Best effort: a failed report only means agents see an older context.
      void client.call({ type: "ReportUiContext", context }).catch(() => undefined);
    }, REPORT_DELAY_MS);
    return () => clearTimeout(timer);
  }, [client, accountId, folderId, messageId, threadId, selectedText, draftId]);

  return null;
}
