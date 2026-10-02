/**
 * "Something waits": a notice for every confirmation an agent or plug-in
 * requests while Nail is open (an effect in the app shell). Each names the
 * actor and the summary and offers "Review", which opens the confirmation
 * center at that confirmation. Notices stay until reviewed, dismissed or
 * decided elsewhere (`ConfirmationResolved`); deciding happens only in the
 * center, never from a notice.
 */
import { useT } from "../../i18n/I18nProvider";
import { useUi } from "../../state/uiStore";
import { Icon } from "../../widgets/Icon";
import { riskOf } from "./ConfirmationCard";
import { attributionMessage } from "./model";
import { CENTER_DIALOG } from "./names";
import { useAgentsState } from "./store";

/** The notices of this UI. */
export function ConfirmationNotices() {
  const t = useT();
  const notices = useAgentsState((s) => s.notices);
  const dismiss = useAgentsState((s) => s.dismiss);
  const setFocus = useAgentsState((s) => s.setFocus);
  const openWindow = useUi((s) => s.openWindow);
  return (
    <section
      data-part="confirmation-notices"
      aria-label={t("agents-notices-label")}
      aria-live="polite"
      hidden={notices.length === 0}
    >
      <ul data-part="confirmation-notices.list">
        {notices.map((confirmation) => {
          const actor = attributionMessage(confirmation.requested_by);
          return (
            <li
              key={confirmation.id}
              data-part="confirmation-notice"
              data-confirmation={confirmation.id}
              data-risk={riskOf(confirmation)}
              data-actor={confirmation.requested_by.actor.kind}
            >
              <div data-part="confirmation-notice.content">
                <span data-part="confirmation-notice.actor">{t(actor.id, actor.args)}</span>
                <strong data-part="confirmation-notice.summary">
                  {t(confirmation.summary.id, confirmation.summary.args)}
                </strong>
              </div>
              <button
                type="button"
                data-part="form.button"
                data-action="review"
                data-variant="primary"
                onClick={() => {
                  setFocus(confirmation.id);
                  dismiss(confirmation.id);
                  openWindow(CENTER_DIALOG);
                }}
              >
                {t("agents-notice-review")}
              </button>
              <button
                type="button"
                data-part="confirmation-notice.dismiss"
                aria-label={t("agents-notice-dismiss")}
                title={t("agents-notice-dismiss")}
                onClick={() => dismiss(confirmation.id)}
              >
                <Icon name="close" part="confirmation-notice.dismiss.icon" />
              </button>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
