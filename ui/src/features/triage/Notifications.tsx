/**
 * New-mail notifications: the settings (widget `notification-settings`:
 * on / off, preview, sound, which folders, per-account and per-folder
 * overrides, quiet hours, a test notification) and, in the browser (the
 * remote UI), showing `NotificationRequested` as a Web Notification
 * (`WebNotifications`, an effect). The desktop shell shows OS notifications
 * itself, from Rust, so the web content never needs the permission there.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useId, useState } from "react";
import { errorMessageOf } from "../../api/client";
import type {
  AccountConfig,
  LocalizedMessage,
  MailNotification,
  NotificationSettings,
  NotifyFolders,
  Weekday,
} from "../../api/generated";
import { useLabel, useT } from "../../i18n/I18nProvider";
import type { WidgetProps } from "../../layout/widgetRegistry";
import { useAccountsConfig, useClient, useFolders } from "../../state/queries";
import { useUiStoreApi } from "../../state/uiStore";
import { CheckField, SelectField } from "../../widgets/forms";
import {
  WEEKDAYS,
  notificationIssues,
  overrideOf,
  withOverride,
  type OverrideChoice,
} from "./model";
import { triageKeys, useNotificationSettings } from "./queries";

const FOLDER_CHOICES: readonly NotifyFolders[] = ["inbox", "all"];
const OVERRIDE_CHOICES: readonly OverrideChoice[] = ["default", "on", "off"];

/** Whether this page may show Web Notifications itself (browser, not the desktop app). */
export function webNotificationsAvailable(): boolean {
  return (
    typeof window !== "undefined" && "Notification" in window && !("__TAURI_INTERNALS__" in window)
  );
}

function Issues({ issues }: { issues: readonly LocalizedMessage[] }) {
  const t = useT();
  if (issues.length === 0) return null;
  return (
    <ul data-part="notification-settings.issues" role="alert">
      {issues.map((issue) => (
        <li key={issue.id} data-part="notification-settings.issue">
          {t(issue.id, issue.args)}
        </li>
      ))}
    </ul>
  );
}

function QuietHoursEditor({
  settings,
  onChange,
}: {
  settings: NotificationSettings;
  onChange(settings: NotificationSettings): void;
}) {
  const t = useT();
  const ids = useId();
  const quiet = settings.quiet_hours;
  return (
    <fieldset data-part="form.section" data-section="quiet-hours">
      <legend data-part="form.section.title">{t("notification-quiet-title")}</legend>
      <CheckField
        labelId="notification-quiet-enabled"
        name="quiet-hours"
        checked={quiet !== null}
        onChange={(on) =>
          onChange({
            ...settings,
            quiet_hours: on ? { start: "22:00", end: "07:00", days: [] } : null,
          })
        }
      />
      {quiet && (
        <>
          <div data-part="notification-settings.times">
            <div data-part="form.field" data-field="quiet-start">
              <label data-part="form.label" htmlFor={`${ids}-start`}>
                {t("notification-quiet-start")}
              </label>
              <input
                id={`${ids}-start`}
                data-part="form.input"
                type="time"
                value={quiet.start}
                onChange={(e) =>
                  onChange({ ...settings, quiet_hours: { ...quiet, start: e.target.value } })
                }
              />
            </div>
            <div data-part="form.field" data-field="quiet-end">
              <label data-part="form.label" htmlFor={`${ids}-end`}>
                {t("notification-quiet-end")}
              </label>
              <input
                id={`${ids}-end`}
                data-part="form.input"
                type="time"
                value={quiet.end}
                onChange={(e) =>
                  onChange({ ...settings, quiet_hours: { ...quiet, end: e.target.value } })
                }
              />
            </div>
          </div>
          <fieldset data-part="notification-settings.days">
            <legend data-part="form.label">{t("notification-quiet-days")}</legend>
            {WEEKDAYS.map((day: Weekday) => (
              <CheckField
                key={day}
                labelId={`notification-day-${day}`}
                name={`day-${day}`}
                checked={quiet.days.includes(day)}
                onChange={(on) =>
                  onChange({
                    ...settings,
                    quiet_hours: {
                      ...quiet,
                      days: on
                        ? WEEKDAYS.filter((d) => d === day || quiet.days.includes(d))
                        : quiet.days.filter((d) => d !== day),
                    },
                  })
                }
              />
            ))}
            <p data-part="form.hint">{t("notification-quiet-days-hint")}</p>
          </fieldset>
        </>
      )}
    </fieldset>
  );
}

function AccountOverrides({
  account,
  settings,
  onChange,
}: {
  account: AccountConfig;
  settings: NotificationSettings;
  onChange(settings: NotificationSettings): void;
}) {
  const folders = useFolders(account.id);
  const choices = OVERRIDE_CHOICES.map((c) => ({
    value: c,
    label: `notification-override-${c}`,
  }));
  const overridden = (folders.data ?? []).filter(
    (f) => overrideOf(settings, { type: "folder", folder_id: f.id }) !== "default",
  );
  const [adding, setAdding] = useState("");
  return (
    <li data-part="notification-settings.account" data-account={account.id}>
      <SelectField
        labelId="notification-override-account"
        args={{ account: account.name }}
        name={`account-${account.id}`}
        value={overrideOf(settings, { type: "account", account_id: account.id })}
        choices={choices}
        onChange={(value) =>
          onChange(
            withOverride(
              settings,
              { type: "account", account_id: account.id },
              value as OverrideChoice,
            ),
          )
        }
      />
      {overridden.map((folder) => (
        <SelectField
          key={folder.id}
          labelId="notification-override-folder"
          args={{ folder: folder.name, account: account.name }}
          name={`folder-${folder.id}`}
          value={overrideOf(settings, { type: "folder", folder_id: folder.id })}
          choices={choices}
          onChange={(value) =>
            onChange(
              withOverride(
                settings,
                { type: "folder", folder_id: folder.id },
                value as OverrideChoice,
              ),
            )
          }
        />
      ))}
      {(folders.data ?? []).length > overridden.length && (
        <SelectField
          labelId="notification-override-add"
          args={{ account: account.name }}
          name={`add-${account.id}`}
          value={adding}
          choices={[
            { value: "", label: "notification-override-add-choose" },
            ...(folders.data ?? [])
              .filter((f) => !overridden.includes(f))
              .map((f) => ({ value: f.id, label: f.name, literal: true })),
          ]}
          onChange={(folderId) => {
            setAdding("");
            if (folderId === "") return;
            // A new folder override flips the folder away from what it does now.
            const notifies =
              settings.folders === "all" ||
              folders.data?.find((f) => f.id === folderId)?.role === "inbox";
            onChange(
              withOverride(
                settings,
                { type: "folder", folder_id: folderId },
                notifies ? "off" : "on",
              ),
            );
          }}
        />
      )}
    </li>
  );
}

function Editor({
  settings: saved,
  notice,
  setNotice,
}: {
  settings: NotificationSettings;
  /** Kept by the widget: a save re-creates the editor. */
  notice: string | null;
  setNotice(id: string | null): void;
}) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const config = useAccountsConfig();
  const [draft, setDraft] = useState(saved);
  const [problems, setProblems] = useState<LocalizedMessage[]>([]);
  const [permission, setPermission] = useState(() =>
    webNotificationsAvailable() ? Notification.permission : null,
  );

  const change = (next: NotificationSettings) => {
    setDraft(next);
    setNotice(null);
  };

  const save = async () => {
    const issues = notificationIssues(draft);
    if (issues.length > 0) {
      setProblems(issues.map((i) => ({ id: `notification-issue-${i}` })));
      return;
    }
    setProblems([]);
    try {
      const answer = await client.request(
        { type: "UpdateNotificationSettings", settings: draft },
        "NotificationSettings",
      );
      setNotice("notification-settings-saved");
      queryClient.setQueryData(triageKeys.notifications, answer.settings);
    } catch (error) {
      setProblems([errorMessageOf(error)]);
    }
  };

  const test = async () => {
    try {
      await client.request({ type: "TestNotification" }, "Done");
      setNotice("notification-test-sent");
    } catch (error) {
      setProblems([errorMessageOf(error)]);
    }
  };

  const askPermission = async () => {
    try {
      setPermission(await Notification.requestPermission());
    } catch {
      setPermission(Notification.permission);
    }
  };

  return (
    <>
      <fieldset data-part="form.section" data-section="general">
        <legend data-part="form.section.title">{t("notification-general-title")}</legend>
        <CheckField
          labelId="notification-enabled"
          name="enabled"
          checked={draft.enabled}
          onChange={(enabled) => change({ ...draft, enabled })}
        />
        <CheckField
          labelId="notification-show-preview"
          name="show-preview"
          checked={draft.show_preview}
          onChange={(show_preview) => change({ ...draft, show_preview })}
        />
        <CheckField
          labelId="notification-sound"
          name="sound"
          checked={draft.sound}
          onChange={(sound) => change({ ...draft, sound })}
        />
        <SelectField
          labelId="notification-folders"
          name="folders"
          value={draft.folders}
          choices={FOLDER_CHOICES.map((f) => ({ value: f, label: `notification-folders-${f}` }))}
          onChange={(folders) => change({ ...draft, folders: folders as NotifyFolders })}
        />
      </fieldset>
      <QuietHoursEditor settings={draft} onChange={change} />
      <fieldset data-part="form.section" data-section="overrides">
        <legend data-part="form.section.title">{t("notification-overrides-title")}</legend>
        <p data-part="form.hint">{t("notification-overrides-hint")}</p>
        <ul data-part="notification-settings.accounts">
          {(config.data?.accounts ?? []).map((account) => (
            <AccountOverrides
              key={account.id}
              account={account}
              settings={draft}
              onChange={change}
            />
          ))}
        </ul>
      </fieldset>
      {permission !== null && permission !== "granted" && (
        <div data-part="notification-settings.permission">
          <p data-part="form.hint">
            {t(
              permission === "denied"
                ? "notification-browser-denied"
                : "notification-browser-needed",
            )}
          </p>
          {permission === "default" && (
            <button type="button" data-part="form.button" onClick={() => void askPermission()}>
              {t("notification-browser-allow")}
            </button>
          )}
        </div>
      )}
      <Issues issues={problems} />
      {notice && (
        <p data-part="notification-settings.notice" role="status">
          {t(notice)}
        </p>
      )}
      <div data-part="notification-settings.actions">
        <button
          type="button"
          data-part="form.button"
          data-variant="primary"
          onClick={() => void save()}
        >
          {t("notification-settings-save")}
        </button>
        <button type="button" data-part="form.button" onClick={() => void test()}>
          {t("notification-test")}
        </button>
      </div>
    </>
  );
}

/** Settings → Notifications (widget `notification-settings`). */
export function NotificationSettingsWidget({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const settings = useNotificationSettings();
  const [notice, setNotice] = useState<string | null>(null);
  let content;
  if (settings.isError) {
    const message = errorMessageOf(settings.error);
    content = (
      <p data-part="notification-settings.error" role="alert">
        {t(message.id, message.args)}
      </p>
    );
  } else if (!settings.data) {
    content = <p data-part="notification-settings.loading">{t("notification-settings-loading")}</p>;
  } else {
    content = (
      <Editor
        key={JSON.stringify(settings.data)}
        settings={settings.data}
        notice={notice}
        setNotice={setNotice}
      />
    );
  }
  return (
    <section
      data-part="notification-settings"
      data-node-id={node.id ?? undefined}
      aria-label={node.label ? label(node.label) : t("notification-settings-label")}
    >
      {content}
    </section>
  );
}

/** Title and body of a notification in the UI language. */
export function notificationTexts(
  notification: MailNotification,
  t: (id: string, args?: Record<string, string | number>) => string,
): { title: string; body: string | undefined } {
  const text = (m: LocalizedMessage) => t(m.id, m.args);
  return {
    title: text(notification.title),
    body: notification.body === null ? undefined : text(notification.body),
  };
}

/**
 * Shows `NotificationRequested` as a Web Notification in the browser when
 * the user allowed it (the remote UI). Clicking it focuses the page and
 * opens the newest message.
 */
export function WebNotifications() {
  const t = useT();
  const client = useClient();
  const store = useUiStoreApi();
  useEffect(() => {
    if (!webNotificationsAvailable()) return;
    return client.subscribe((event) => {
      if (event.type !== "NotificationRequested") return;
      if (Notification.permission !== "granted") return;
      const n = event.notification;
      const { title, body } = notificationTexts(n, t);
      try {
        const shown = new Notification(title, {
          ...(body === undefined ? {} : { body }),
          tag: n.tag,
          silent: !n.sound,
        });
        shown.onclick = () => {
          window.focus();
          const first = n.message_ids[0];
          if (first !== undefined) store.getState().selectMessage(first);
          shown.close();
        };
      } catch (error) {
        console.warn("nail: cannot show a notification", error);
      }
    });
  }, [client, store, t]);
  return null;
}
