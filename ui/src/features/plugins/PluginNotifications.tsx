/**
 * Plug-in notifications (`nail.ui.notify`, ADR 0050) as in-app toasts, and
 * notices about plug-ins the host stopped (crash, CPU budget, memory
 * limit). Each toast names its plug-in. Errors stay until dismissed;
 * everything else disappears after `TOAST_MS`. `system` notifications are
 * also shown as Web Notifications in the browser (the desktop shell shows
 * OS notifications; `handlePluginsEvent`).
 */
import { useEffect } from "react";
import { useT } from "../../i18n/I18nProvider";
import { Icon } from "../../widgets/Icon";
import { usePluginName } from "./PluginDialogs";
import { partAttrs } from "./parts";
import { usePluginsState, type Toast } from "./store";

/** How long a toast stays (ms). */
export const TOAST_MS = 8000;

function levelOf(toast: Toast) {
  return toast.kind === "plugin" ? (toast.notification.spec.level ?? "info") : toast.level;
}

function pluginOf(toast: Toast): string {
  return toast.kind === "plugin" ? toast.notification.plugin_id : toast.pluginId;
}

function ToastView({ toast }: { toast: Toast }) {
  const t = useT();
  const dismiss = usePluginsState((s) => s.dismissToast);
  const plugin = pluginOf(toast);
  const name = usePluginName(plugin);
  const level = levelOf(toast);

  useEffect(() => {
    if (level === "error") return;
    const timer = setTimeout(() => dismiss(toast.id), TOAST_MS);
    return () => clearTimeout(timer);
  }, [dismiss, level, toast.id]);

  return (
    <li {...partAttrs("plugin.notification", plugin)} data-level={level}>
      <div
        {...partAttrs("plugin.notification.content", plugin)}
        role={level === "error" ? "alert" : undefined}
      >
        <span {...partAttrs("plugin.notification.plugin", plugin)}>{name}</span>
        {toast.kind === "plugin" ? (
          <>
            <strong {...partAttrs("plugin.notification.title", plugin)}>
              {toast.notification.spec.title}
            </strong>
            {toast.notification.spec.body !== undefined && (
              <span {...partAttrs("plugin.notification.body", plugin)}>
                {toast.notification.spec.body}
              </span>
            )}
          </>
        ) : (
          <strong {...partAttrs("plugin.notification.title", plugin)}>
            {t(toast.message.id, { ...toast.message.args, plugin: name })}
          </strong>
        )}
      </div>
      <button
        type="button"
        {...partAttrs("plugin.notification.dismiss", plugin)}
        aria-label={t("plugin-notification-dismiss")}
        title={t("plugin-notification-dismiss")}
        onClick={() => dismiss(toast.id)}
      >
        <Icon name="close" part="plugin.notification.dismiss.icon" />
      </button>
    </li>
  );
}

/** The toasts of this UI (an effect in the app shell). */
export function PluginNotifications() {
  const t = useT();
  const toasts = usePluginsState((s) => s.toasts);
  return (
    <section
      data-part="plugin.notifications"
      aria-label={t("plugin-notifications-label")}
      aria-live="polite"
      hidden={toasts.length === 0}
    >
      <ul data-part="plugin.notifications.list">
        {toasts.map((toast) => (
          <ToastView key={toast.id} toast={toast} />
        ))}
      </ul>
    </section>
  );
}
