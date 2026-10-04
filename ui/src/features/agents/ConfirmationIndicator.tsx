/**
 * The confirmation indicator (layout widget `confirmation-indicator`, for
 * the status bar or a toolbar): how many writes of agents and plug-ins
 * wait for the user; pressing it opens the confirmation center.
 *
 * Widget options: `hide-when-empty` (default `#true`).
 */
import { useT } from "../../i18n/I18nProvider";
import { optionBool, type WidgetProps } from "../../layout/widgetRegistry";
import { useUi } from "../../state/uiStore";
import { CENTER_DIALOG } from "./names";
import { usePendingConfirmations } from "./queries";
import { isDestructive } from "./model";

/** Widget `confirmation-indicator`. */
export function ConfirmationIndicator({ node }: WidgetProps) {
  const t = useT();
  const openWindow = useUi((s) => s.openWindow);
  const pending = usePendingConfirmations();
  const list = pending.data ?? [];
  const count = list.length;
  if (count === 0 && optionBool(node, "hide-when-empty", true)) return null;
  const destructive = list.some((c) => isDestructive(c.kind));
  return (
    <button
      type="button"
      data-part="confirmation-indicator"
      data-node-id={node.id ?? undefined}
      data-state={count > 0 ? "waiting" : "idle"}
      data-risk={destructive ? "destructive" : undefined}
      aria-label={t("agents-indicator-label", { count })}
      title={t("agents-indicator-label", { count })}
      onClick={() => {
        openWindow(CENTER_DIALOG);
      }}
    >
      <span data-part="confirmation-indicator.count" aria-hidden="true">
        {t("agents-indicator-count", { count })}
      </span>
    </button>
  );
}
