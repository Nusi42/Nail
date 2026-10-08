/**
 * Security alerts of the remote UI (`RemoteSecurityAlert`): a new device
 * signed in, or a name / address was locked out after failed logins.
 * Shown as toasts that stay until acknowledged (they matter), each with
 * "review devices" (opens Settings) and "dismiss"; Escape dismisses the
 * focused one. Mounted once in the app shell (feature effect).
 */
import { useLocale, useT } from "../../i18n/I18nProvider";
import { useUiStoreApi } from "../../state/uiStore";
import { formatTime } from "./model";
import { remoteStore, useRemoteState, type ShownAlert } from "./state";
import { SETTINGS_WINDOW } from "../settings/model";

/** The layout window that holds the remote-access settings. */

function Toast({ shown }: { shown: ShownAlert }) {
  const t = useT();
  const locale = useLocale();
  const uiStore = useUiStoreApi();
  const { alert } = shown;
  const dismiss = () => {
    remoteStore.getState().dismissAlert(shown.id);
  };
  const text =
    alert.kind === "new-device"
      ? t(alert.ip === null ? "remote-alert-new-device" : "remote-alert-new-device-ip", {
          device: alert.device_name,
          user: alert.user_name,
          ip: alert.ip ?? "",
        })
      : t(
          alert.user_name !== null
            ? "remote-alert-lockout-user"
            : alert.ip !== null
              ? "remote-alert-lockout-ip"
              : "remote-alert-lockout",
          {
            user: alert.user_name ?? "",
            ip: alert.ip ?? "",
            until: formatTime(alert.until, locale),
          },
        );
  return (
    // eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions -- Escape from the alert's buttons dismisses the alert.
    <div
      data-part="remote-alerts.toast"
      data-kind={alert.kind}
      role="alert"
      aria-label={t(`remote-alert-title-${alert.kind}`)}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.stopPropagation();
          dismiss();
        }
      }}
    >
      <p data-part="remote-alerts.title">{t(`remote-alert-title-${alert.kind}`)}</p>
      <p data-part="remote-alerts.text">{text}</p>
      <div data-part="remote-alerts.actions">
        <button
          type="button"
          data-part="form.button"
          data-action="review-devices"
          onClick={() => {
            uiStore.getState().openWindow(SETTINGS_WINDOW);
            dismiss();
          }}
        >
          {t("remote-alert-review")}
        </button>
        <button type="button" data-part="form.button" data-action="dismiss" onClick={dismiss}>
          {t("common-dismiss")}
        </button>
      </div>
    </div>
  );
}

/** The toast stack (feature effect). */
export function RemoteAlerts() {
  const t = useT();
  const alerts = useRemoteState((s) => s.alerts);
  if (alerts.length === 0) return null;
  return (
    <section data-part="remote-alerts" aria-label={t("remote-alerts-label")}>
      {alerts.map((a) => (
        <Toast key={a.id} shown={a} />
      ))}
    </section>
  );
}
