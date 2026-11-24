/**
 * A toast with one line of text, the actions of its kind and Close: the
 * problem toast of a refused PIM write and the toast of an invitation
 * answer waiting in the outbox (R8). Reminder toasts have their own layout.
 */
import type { ReactNode } from "react";
import { useT } from "../../i18n/I18nProvider";
import { usePim } from "./store";

export function TextToast({
  id,
  kind,
  state,
  role,
  text,
  children,
}: {
  /** The toast's id (Close dismisses it). */
  id: string;
  /** `data-kind` of the toast. */
  kind: string;
  /** `data-state` of the toast, if any. */
  state?: string;
  /** `alert` for problems, `status` for news. */
  role: "alert" | "status";
  text: string;
  /** Actions before Close. */
  children?: ReactNode;
}) {
  const t = useT();
  const dismissToast = usePim((s) => s.dismissToast);
  return (
    <li data-part="pim.toast" data-kind={kind} data-state={state}>
      <p data-part="pim.toast.title" role={role}>
        {text}
      </p>
      <div data-part="pim.toast.actions">
        {children}
        <button
          type="button"
          data-part="form.button"
          onClick={() => {
            dismissToast(id);
          }}
        >
          {t("common-close")}
        </button>
      </div>
    </li>
  );
}
