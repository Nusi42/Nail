/**
 * Settings → Remote access (layout widget `remote-settings`): serve the
 * remote UI or not, where (loopback, the LAN or one address), TLS, the
 * session limits and allowed networks, with the running state, the
 * addresses and the certificate fingerprint. Internet mode is shown as
 * locked, with the reason (`internet_mode_lock_reason`).
 *
 * The form checks what the core checks (`settingsIssues`) and names the
 * problem before saving; the core's answer wins.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { NailError, errorMessageId, errorMessageIdOf } from "../../api/client";
import type {
  RemoteTlsMode,
  RemoteUiSettings,
  RemoteUiSettingsIssue,
  RemoteUiStatus,
} from "../../api/generated";
import { useLabel, useLocale, useT } from "../../i18n/I18nProvider";
import type { WidgetProps } from "../../layout/widgetRegistry";
import { useClient } from "../../state/queries";
import { NumberField, SelectField, TextAreaField } from "../../widgets/forms";
import { useSettingsForm } from "../settings/draft";
import { ConfirmButton, InputField, Toggle } from "./fields";
import {
  ALL_INTERFACES,
  BIND_CHOICES,
  LOOPBACK_ADDRESS,
  TLS_CHOICES,
  bindChoiceOf,
  formatTime,
  groupFingerprint,
  isLoopbackAddress,
  issueOfMessage,
  settingsIssues,
  type BindChoice,
} from "./model";
import { remoteKeys, useRemoteUi, type RemoteUi } from "./queries";
import { sameJson } from "../../text/json";

/** The allowed networks as the text area shows them (one per line). */
export function networksText(networks: readonly string[]): string {
  return networks.join("\n");
}

/** The text area's lines as networks (blank lines dropped). */
export function parseNetworks(text: string): string[] {
  return text
    .split(/[\n,]/)
    .map((l) => l.trim())
    .filter((l) => l !== "");
}

function Status({ status }: { status: RemoteUiStatus }) {
  const t = useT();
  const locale = useLocale();
  const client = useClient();
  const queryClient = useQueryClient();
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState(false);
  const cert = status.certificate;

  const regenerate = async () => {
    setError(null);
    setDone(false);
    try {
      const answer = await client.request({ type: "RegenerateRemoteCertificate" }, "RemoteUi");
      queryClient.setQueryData<RemoteUi>(remoteKeys.ui, {
        settings: answer.settings,
        status: answer.status,
      });
      setDone(true);
    } catch (e) {
      setError(errorMessageIdOf(e));
    }
  };

  return (
    <section
      data-part="remote-settings.status"
      data-state={status.state}
      aria-label={t("remote-status-title")}
    >
      <h3 data-part="remote-settings.title">{t("remote-status-title")}</h3>
      <p data-part="remote-settings.state" data-state={status.state} role="status">
        {t(`remote-state-${status.state}`)}
      </p>
      {status.state === "failed" && status.error !== null && (
        <p data-part="remote-settings.error" role="alert">
          {t("remote-state-failed-because", { error: t(errorMessageId(status.error)) })}
        </p>
      )}
      {status.urls.length > 0 && (
        <ul data-part="remote-settings.urls" aria-label={t("remote-status-urls")}>
          {status.urls.map((url) => (
            <li key={url} data-part="remote-settings.url">
              <code data-part="remote-settings.url.text">{url}</code>
            </li>
          ))}
        </ul>
      )}
      {status.state === "running" && (
        <p data-part="remote-settings.sessions">
          {t("remote-status-sessions", { count: status.active_sessions })}
        </p>
      )}
      {cert !== null && (
        <div data-part="remote-settings.certificate" data-source={cert.source}>
          <p data-part="remote-settings.certificate.title">
            {t(`remote-certificate-${cert.source}`)}
          </p>
          <p data-part="remote-settings.certificate.label">{t("remote-fingerprint-label")}</p>
          <code data-part="remote-fingerprint">{groupFingerprint(cert.fingerprint_sha256)}</code>
          <p data-part="form.hint">
            {t("remote-certificate-valid", {
              from: formatTime(cert.not_before, locale),
              until: formatTime(cert.not_after, locale),
            })}
          </p>
          {cert.subject_names.length > 0 && (
            <p data-part="form.hint">
              {t("remote-certificate-names", { names: cert.subject_names.join(", ") })}
            </p>
          )}
          {cert.source === "self-signed" && (
            <ConfirmButton
              labelId="remote-certificate-regenerate"
              questionId="remote-certificate-regenerate-confirm"
              yesId="remote-certificate-regenerate-yes"
              action="regenerate-certificate"
              onConfirm={regenerate}
            />
          )}
          {done && (
            <p data-part="remote-settings.saved" role="status">
              {t("remote-certificate-regenerated")}
            </p>
          )}
        </div>
      )}
      {error && (
        <p data-part="remote-settings.error" role="alert">
          {t(error)}
        </p>
      )}
    </section>
  );
}

function Internet({ status }: { status: RemoteUiStatus }) {
  const t = useT();
  const reason = status.internet_mode_locked
    ? t(`remote-internet-lock-${status.internet_mode_lock_reason ?? "pentest-pending"}`)
    : null;
  return (
    <section data-part="remote-settings.internet" aria-label={t("remote-internet-title")}>
      <h3 data-part="remote-settings.title">{t("remote-internet-title")}</h3>
      <p data-part="remote-settings.intro">{t("remote-internet-intro")}</p>
      <Toggle
        labelId="remote-internet-mode"
        name="internet-mode"
        checked={false}
        onChange={() => undefined}
        lockedReason={reason}
      />
      <p data-part="form.hint">{t("remote-internet-advice")}</p>
    </section>
  );
}

/**
 * What the form edits: the settings, the network choice and the allowed
 * networks as typed (one per line).
 */
interface RemoteDraft {
  settings: RemoteUiSettings;
  bind: BindChoice;
  networks: string;
}

/** The settings a draft would save. */
function settingsOf(draft: RemoteDraft): RemoteUiSettings {
  return { ...draft.settings, allowed_networks: parseNetworks(draft.networks) };
}

/** The form's values for saved settings. */
function draftOf(saved: RemoteUiSettings): RemoteDraft {
  return {
    settings: saved,
    bind: bindChoiceOf(saved.bind_address),
    networks: networksText(saved.allowed_networks),
  };
}

function SettingsForm({ saved, status }: { saved: RemoteUiSettings; status: RemoteUiStatus }) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const [notice, setNotice] = useState<{ id: string; error: boolean } | null>(null);
  const [serverIssue, setServerIssue] = useState<RemoteUiSettingsIssue | null>(null);
  // R6 (ADR 0239): the edits wait for the settings page's Save.
  const form = useSettingsForm<RemoteDraft>(draftOf(saved), {
    equal: (a, b) => sameJson(settingsOf(a), settingsOf(b)),
    canSave: (value) => settingsIssues(settingsOf(value)).length === 0,
    save: async (value) => {
      const answer = await client.request(
        { type: "UpdateRemoteUiSettings", settings: settingsOf(value) },
        "RemoteUi",
      );
      queryClient.setQueryData<RemoteUi>(remoteKeys.ui, {
        settings: answer.settings,
        status: answer.status,
      });
    },
    onSaved: () => {
      setNotice({ id: "remote-settings-saved", error: false });
    },
    onError: (e) => {
      setServerIssue(e instanceof NailError ? issueOfMessage(e.message) : null);
      setNotice({ id: errorMessageIdOf(e), error: true });
    },
  });
  const { settings: draft, bind, networks } = form.value;
  const next = settingsOf(form.value);
  const issues = settingsIssues(next);
  const loopback = isLoopbackAddress(next.bind_address);

  const change = (patch: Partial<RemoteUiSettings>) => {
    form.set((d) => ({ ...d, settings: { ...d.settings, ...patch } }));
    setNotice(null);
    setServerIssue(null);
  };

  const chooseBind = (choice: BindChoice) => {
    form.set((d) => ({ ...d, bind: choice }));
    if (choice === "loopback") change({ bind_address: LOOPBACK_ADDRESS });
    else if (choice === "lan") {
      change({
        bind_address: ALL_INTERFACES,
        tls: draft.tls === "off" ? "self-signed" : draft.tls,
      });
    } else
      change({ bind_address: draft.bind_address === ALL_INTERFACES ? "" : draft.bind_address });
  };

  const shown =
    serverIssue !== null && !issues.includes(serverIssue) ? [...issues, serverIssue] : issues;

  return (
    <form
      data-part="remote-settings.form"
      aria-label={t("remote-settings-title")}
      noValidate
      onSubmit={(e) => {
        e.preventDefault();
      }}
    >
      <p data-part="remote-settings.intro">{t("remote-settings-intro")}</p>
      <Toggle
        labelId="remote-settings-enabled"
        name="enabled"
        checked={draft.enabled}
        onChange={(enabled) => {
          change({ enabled });
        }}
      />
      <fieldset data-part="form.section">
        <legend data-part="form.section.title">{t("remote-settings-network")}</legend>
        <SelectField
          labelId="remote-settings-bind"
          name="bind"
          value={bind}
          onChange={(v) => {
            chooseBind(v as BindChoice);
          }}
          choices={BIND_CHOICES.map((c) => ({ value: c, label: `remote-bind-${c}` }))}
        />
        {bind === "custom" && (
          <InputField
            labelId="remote-settings-bind-address"
            hintId="remote-settings-bind-address-hint"
            name="bind-address"
            value={draft.bind_address}
            onChange={(bind_address) => {
              change({ bind_address: bind_address.trim() });
            }}
            invalid={issues.includes("invalid-bind-address")}
          />
        )}
        {!loopback && draft.enabled && (
          <p data-part="remote-settings.warning" role="note">
            {t("remote-settings-lan-warning")}
          </p>
        )}
        <NumberField
          labelId="remote-settings-port"
          name="port"
          value={draft.port}
          min={1}
          max={65535}
          onChange={(port) => {
            change({ port: port ?? 0 });
          }}
        />
        <InputField
          labelId="remote-settings-host-name"
          hintId="remote-settings-host-name-hint"
          name="host-name"
          value={draft.host_name ?? ""}
          onChange={(h) => {
            change({ host_name: h.trim() === "" ? null : h.trim() });
          }}
          invalid={issues.includes("invalid-host-name")}
        />
        <SelectField
          labelId="remote-settings-tls"
          name="tls"
          value={draft.tls}
          onChange={(tls) => {
            change({ tls: tls as RemoteTlsMode });
          }}
          choices={TLS_CHOICES.map((m) => ({ value: m, label: `remote-tls-${m}` }))}
        />
        <p data-part="form.hint">{t(`remote-tls-${draft.tls}-hint`)}</p>
        <TextAreaField
          labelId="remote-settings-networks"
          name="allowed-networks"
          value={networks}
          onChange={(v) => {
            form.set((d) => ({ ...d, networks: v }));
            setNotice(null);
          }}
        />
        <p data-part="form.hint">{t("remote-settings-networks-hint")}</p>
      </fieldset>
      <fieldset data-part="form.section">
        <legend data-part="form.section.title">{t("remote-settings-sessions")}</legend>
        <NumberField
          labelId="remote-settings-idle"
          name="session-idle"
          value={draft.session_idle_minutes}
          min={1}
          onChange={(v) => {
            change({ session_idle_minutes: v ?? 0 });
          }}
        />
        <NumberField
          labelId="remote-settings-max-hours"
          name="session-max"
          value={draft.session_max_hours}
          min={1}
          onChange={(v) => {
            change({ session_max_hours: v ?? 0 });
          }}
        />
        <NumberField
          labelId="remote-settings-reauth"
          name="reauth"
          value={draft.reauth_minutes}
          min={1}
          onChange={(v) => {
            change({ reauth_minutes: v ?? 0 });
          }}
        />
        <NumberField
          labelId="remote-settings-audit-retention"
          name="audit-retention"
          value={draft.audit_retention_days}
          min={1}
          onChange={(v) => {
            change({ audit_retention_days: v ?? 0 });
          }}
        />
      </fieldset>
      {shown.length > 0 && (
        <ul data-part="remote-settings.issues" aria-label={t("common-problems")}>
          {shown.map((issue) => (
            <li key={issue} data-part="remote-settings.issue" data-issue={issue}>
              {t(`remote-issue-${issue}`)}
            </li>
          ))}
        </ul>
      )}
      {notice && (
        <p
          data-part="remote-settings.notice"
          data-state={notice.error ? "error" : "ok"}
          role={notice.error ? "alert" : "status"}
        >
          {t(notice.id)}
        </p>
      )}
      <Internet status={status} />
    </form>
  );
}

/** The `remote-settings` widget. */
export function RemoteSettingsWidget({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const ui = useRemoteUi();
  return (
    <section
      data-part="remote-settings"
      data-node-id={node.id ?? undefined}
      aria-label={node.label ? label(node.label) : t("remote-settings-title")}
      aria-busy={ui.isFetching}
    >
      {ui.isError && (
        <p data-part="remote-settings.error" role="alert">
          {t(errorMessageIdOf(ui.error))}
        </p>
      )}
      {ui.data && (
        <>
          <Status status={ui.data.status} />
          <SettingsForm saved={ui.data.settings} status={ui.data.status} />
        </>
      )}
    </section>
  );
}
