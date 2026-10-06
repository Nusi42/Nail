/**
 * The undo toast (P3, ADR 0164): what an action did — "Conversation
 * archived", "3 conversations moved to Trash", "Message deleted" — with
 * Undo while the batch can be undone (`mail.undo`, `z`, does the same), or
 * why the core refused it. It hides after a few seconds unless the pointer
 * or the focus is on it.
 *
 * It is a manual popover (top layer, above every window); its place and
 * look come from `MAIL_ACTIONS_SKIN_CSS` and the theme.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useT } from "../../i18n/I18nProvider";
import { useClient } from "../../state/queries";
import { useUiStoreApi } from "../../state/uiStore";
import { Icon } from "../../widgets/Icon";
import { undoLast } from "./run";
import { useMailActions, type MailToast } from "./store";

/** How long a toast stays (ms). */
export const TOAST_MS = { done: 8000, undone: 4000, error: 8000 } as const;

/** The toast's text: Fluent id and arguments. */
export function toastMessage(toast: MailToast): {
  id: string;
  args?: Record<string, number | string>;
} {
  switch (toast.kind) {
    case "done":
      if (toast.tags !== undefined) {
        return {
          id: `mail-actions-done-tags-${toast.tags.change}`,
          args: {
            conversations: toast.conversations,
            messages: toast.messages,
            noun: toast.tags.noun,
            tags: toast.tags.count,
          },
        };
      }
      return {
        id: `mail-actions-done-${toast.action}`,
        args: { conversations: toast.conversations, messages: toast.messages },
      };
    case "undone":
      return { id: "mail-actions-undone" };
    case "error":
      return { id: toast.message.id };
  }
}

/**
 * Whether the browser has popovers (every engine Nail runs in; test DOMs
 * without them show the toast in place, so it never hides).
 */
const POPOVERS = typeof HTMLElement !== "undefined" && "showPopover" in HTMLElement.prototype;

/** Shows the popover in the top layer where the browser supports it. */
function showPopover(element: HTMLElement | null): void {
  if (element === null || !POPOVERS) return;
  try {
    element.showPopover();
  } catch {
    // Already shown.
  }
}

export function MailActionToast() {
  const t = useT();
  const toast = useMailActions((s) => s.toast);
  const dismiss = useMailActions((s) => s.dismiss);
  const client = useClient();
  const store = useUiStoreApi();
  const queryClient = useQueryClient();
  const ref = useRef<HTMLDivElement>(null);
  const [held, setHeld] = useState(false);

  useLayoutEffect(() => {
    if (toast !== null) showPopover(ref.current);
  }, [toast]);

  useEffect(() => {
    if (toast === null || held) return undefined;
    const timer = setTimeout(dismiss, TOAST_MS[toast.kind]);
    return () => {
      clearTimeout(timer);
    };
  }, [toast, held, dismiss]);

  if (toast === null) return null;
  const message = toastMessage(toast);
  const text =
    toast.kind === "error" ? t(toast.message.id, toast.message.args) : t(message.id, message.args);
  const batchId = toast.kind === "done" ? toast.batchId : null;
  return (
    <div
      ref={ref}
      popover={POPOVERS ? "manual" : undefined}
      data-part="mail-actions.toast"
      data-kind={toast.kind}
      data-action={toast.kind === "done" ? toast.action : undefined}
      role={toast.kind === "error" ? "alert" : "status"}
      onPointerEnter={() => {
        setHeld(true);
      }}
      onPointerLeave={() => {
        setHeld(false);
      }}
      onFocus={() => {
        setHeld(true);
      }}
      onBlur={() => {
        setHeld(false);
      }}
    >
      <span data-part="mail-actions.toast.text">{text}</span>
      {batchId !== null && (
        <button
          type="button"
          data-part="mail-actions.toast.undo"
          onClick={() => {
            dismiss();
            void undoLast({ client, store, queryClient }, batchId);
          }}
        >
          {t("mail-actions-undo")}
        </button>
      )}
      <button
        type="button"
        data-part="mail-actions.toast.dismiss"
        aria-label={t("mail-actions-dismiss")}
        title={t("mail-actions-dismiss")}
        onClick={dismiss}
      >
        <Icon name="x" part="mail-actions.toast.dismiss.icon" />
      </button>
    </div>
  );
}
