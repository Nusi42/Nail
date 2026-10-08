/**
 * Settings → Remote access → Devices and sessions (layout widget
 * `remote-devices`): paired browsers (rename, revoke — a revoked device
 * must pair again), the sessions signed in now (or also ended ones) with
 * how they signed in, "sign out" per session and for everyone, and the
 * pairing codes still waiting to be used.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useState, type SubmitEvent } from "react";
import { errorMessageIdOf } from "../../api/client";
import type { PairingCode, RemoteDevice, RemoteSession, RemoteUser } from "../../api/generated";
import { useLabel, useLocale, useT } from "../../i18n/I18nProvider";
import type { WidgetProps } from "../../layout/widgetRegistry";
import { useClient } from "../../state/queries";
import { useUiStoreApi } from "../../state/uiStore";
import { ConfirmButton, InputField, Toggle, RenameActions } from "./fields";
import { formatTime, presetOf } from "./model";
import { PAIR_DIALOG } from "./PairDevice";
import {
  remoteKeys,
  usePairingCodes,
  useRemoteDevices,
  useRemoteSessions,
  useRemoteUsers,
} from "./queries";
import { remoteStore } from "./state";

function useRun() {
  const client = useClient();
  const queryClient = useQueryClient();
  const [error, setError] = useState<string | null>(null);
  const run = async (fn: () => Promise<unknown>): Promise<boolean> => {
    setError(null);
    try {
      await fn();
      await queryClient.invalidateQueries({ queryKey: remoteKeys.all });
      return true;
    } catch (e) {
      setError(errorMessageIdOf(e));
      return false;
    }
  };
  return { client, error, run };
}

function ErrorLine({ error }: { error: string | null }) {
  const t = useT();
  if (error === null) return null;
  return (
    <p data-part="remote-devices.error" role="alert">
      {t(error)}
    </p>
  );
}

function userName(users: readonly RemoteUser[], id: string): string {
  const u = users.find((x) => x.id === id);
  return u === undefined ? id : (u.display_name ?? u.name);
}

function DeviceRow({ device, users }: { device: RemoteDevice; users: readonly RemoteUser[] }) {
  const t = useT();
  const locale = useLocale();
  const { client, error, run } = useRun();
  const [name, setName] = useState<string | null>(null);
  const revoked = device.revoked_at !== null;
  const preset = presetOf(device.grant);

  const rename = async (event: SubmitEvent) => {
    event.preventDefault();
    if (name === null || name.trim() === "") return;
    const ok = await run(() =>
      client.request(
        { type: "UpdateRemoteDevice", device_id: device.id, name: name.trim(), grant: null },
        "RemoteDeviceSaved",
      ),
    );
    if (ok) setName(null);
  };

  return (
    <li
      data-part="remote-devices.item"
      data-device={device.id}
      data-state={revoked ? "revoked" : "active"}
      aria-label={device.name}
    >
      {name === null ? (
        <span data-part="remote-devices.name">{device.name}</span>
      ) : (
        <form data-part="remote-devices.rename" noValidate onSubmit={(e) => void rename(e)}>
          <InputField
            labelId="remote-device-new-name"
            name="device-name"
            value={name}
            onChange={setName}
            focusOnMount
          />
          <RenameActions
            disabled={name.trim() === ""}
            onCancel={() => {
              setName(null);
            }}
          />
        </form>
      )}
      <span data-part="remote-devices.meta">
        {t("remote-device-meta", {
          user: userName(users, device.user_id),
          paired: formatTime(device.paired_at, locale),
          seen:
            device.last_seen_at === null
              ? t("common-never-inline")
              : formatTime(device.last_seen_at, locale),
          ip: device.last_ip ?? "",
        })}
      </span>
      <span data-part="remote-devices.access" data-preset={preset}>
        {t(`remote-preset-${preset}`)}
      </span>
      {device.grant.admin && (
        <span data-part="remote-devices.admin">{t("remote-access-admin-short")}</span>
      )}
      {device.user_agent !== null && (
        <span data-part="remote-devices.agent">{device.user_agent}</span>
      )}
      {revoked ? (
        <span data-part="remote-devices.badge" data-state="revoked">
          {t("remote-device-revoked", { time: formatTime(device.revoked_at, locale) })}
        </span>
      ) : (
        name === null && (
          <span data-part="remote-devices.actions">
            <button
              type="button"
              data-part="form.button"
              data-action="rename-device"
              aria-label={t("remote-device-rename-label", { name: device.name })}
              onClick={() => {
                setName(device.name);
              }}
            >
              {t("common-rename")}
            </button>
            <ConfirmButton
              labelId="remote-device-revoke"
              labelArgs={{ name: device.name }}
              questionId="remote-device-revoke-confirm"
              questionArgs={{ name: device.name }}
              yesId="remote-device-revoke-yes"
              action="revoke-device"
              onConfirm={() =>
                run(() =>
                  client.request({ type: "RevokeRemoteDevice", device_id: device.id }, "Done"),
                )
              }
            />
          </span>
        )
      )}
      <ErrorLine error={error} />
    </li>
  );
}

function SessionRow({ session }: { session: RemoteSession }) {
  const t = useT();
  const locale = useLocale();
  const { client, error, run } = useRun();
  const ended = session.ended_at !== null;
  return (
    <li
      data-part="remote-sessions.item"
      data-session={session.id}
      data-state={ended ? "ended" : "active"}
      aria-label={t("remote-session-label", {
        user: session.user_name,
        device: session.device_name,
      })}
    >
      <span data-part="remote-sessions.who">
        {t("remote-session-who", { user: session.user_name, device: session.device_name })}
      </span>
      <span data-part="remote-sessions.method" data-method={session.auth_method}>
        {t(`remote-method-${session.auth_method}`)}
      </span>
      <span data-part="remote-sessions.meta">
        {t("remote-session-meta", {
          started: formatTime(session.created_at, locale),
          seen: formatTime(session.last_seen_at, locale),
          ip: session.ip ?? "",
        })}
      </span>
      {ended ? (
        <span data-part="remote-sessions.ended" data-reason={session.end_reason ?? undefined}>
          {t("remote-session-ended", {
            time: formatTime(session.ended_at, locale),
            reason: t(`remote-end-${session.end_reason ?? "expired"}`),
          })}
        </span>
      ) : (
        <>
          <span data-part="remote-sessions.expires">
            {t("remote-session-expires", { time: formatTime(session.expires_at, locale) })}
          </span>
          <button
            type="button"
            data-part="form.button"
            data-action="revoke-session"
            data-variant="danger"
            aria-label={t("remote-session-revoke-label", {
              user: session.user_name,
              device: session.device_name,
            })}
            onClick={() =>
              void run(() =>
                client.request({ type: "RevokeRemoteSession", session_id: session.id }, "Done"),
              )
            }
          >
            {t("remote-action-logout")}
          </button>
        </>
      )}
      <ErrorLine error={error} />
    </li>
  );
}

function PendingCode({ code, users }: { code: PairingCode; users: readonly RemoteUser[] }) {
  const t = useT();
  const locale = useLocale();
  const { client, error, run } = useRun();
  return (
    <li data-part="remote-pairing.item" data-pairing={code.id}>
      <span data-part="remote-pairing.text">
        {t(code.device_name === null ? "remote-pairing-pending-any" : "remote-pairing-pending", {
          user: userName(users, code.user_id),
          device: code.device_name ?? "",
          time: formatTime(code.expires_at, locale),
        })}
      </span>
      <button
        type="button"
        data-part="form.button"
        data-action="cancel-code"
        onClick={() =>
          void run(() => client.request({ type: "CancelPairingCode", pairing_id: code.id }, "Done"))
        }
      >
        {t("remote-pair-cancel-code")}
      </button>
      <ErrorLine error={error} />
    </li>
  );
}

/** The `remote-devices` widget. */
export function RemoteDevicesWidget({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const uiStore = useUiStoreApi();
  const [includeEnded, setIncludeEnded] = useState(false);
  const users = useRemoteUsers().data ?? [];
  const devices = useRemoteDevices();
  const sessions = useRemoteSessions(includeEnded);
  const codes = usePairingCodes();
  const { client, error, run } = useRun();
  const pending = (codes.data ?? []).filter((c) => c.state === "pending");
  const deviceList = devices.data ?? [];
  const sessionList = sessions.data ?? [];
  const active = sessionList.filter((s) => s.ended_at === null).length;
  const loadError = devices.error ?? sessions.error ?? codes.error;

  return (
    <section
      data-part="remote-devices"
      data-node-id={node.id ?? undefined}
      aria-label={node.label ? label(node.label) : t("remote-devices-title")}
    >
      {loadError && <ErrorLine error={errorMessageIdOf(loadError)} />}
      {/* Not a second landmark: the widget itself is the "Devices" region. */}
      <div data-part="remote-devices.section">
        <h3 data-part="remote-devices.title">{t("remote-devices-title")}</h3>
        <button
          type="button"
          data-part="form.button"
          data-variant="primary"
          data-action="pair-device"
          disabled={users.every((u) => u.disabled)}
          onClick={() => {
            remoteStore.getState().setPairUser(null);
            uiStore.getState().openWindow(PAIR_DIALOG);
          }}
        >
          {t("remote-pair-open-dialog")}
        </button>
        {devices.isSuccess && deviceList.length === 0 && (
          <p data-part="remote-devices.empty">{t("remote-devices-empty")}</p>
        )}
        {deviceList.length > 0 && (
          <ul data-part="remote-devices.list" aria-label={t("remote-devices-title")}>
            {deviceList.map((d) => (
              <DeviceRow key={d.id} device={d} users={users} />
            ))}
          </ul>
        )}
        {pending.length > 0 && (
          <>
            <h4 data-part="remote-devices.subtitle">{t("remote-pairing-title")}</h4>
            <ul data-part="remote-pairing" aria-label={t("remote-pairing-title")}>
              {pending.map((c) => (
                <PendingCode key={c.id} code={c} users={users} />
              ))}
            </ul>
          </>
        )}
      </div>
      <section data-part="remote-sessions" aria-label={t("remote-sessions-title")}>
        <h3 data-part="remote-devices.title">{t("remote-sessions-title")}</h3>
        <Toggle
          labelId="remote-sessions-include-ended"
          name="include-ended"
          checked={includeEnded}
          onChange={setIncludeEnded}
        />
        {sessions.isSuccess && sessionList.length === 0 && (
          <p data-part="remote-sessions.empty">{t("remote-sessions-empty")}</p>
        )}
        {sessionList.length > 0 && (
          <ul data-part="remote-sessions.list" aria-label={t("remote-sessions-title")}>
            {sessionList.map((s) => (
              <SessionRow key={s.id} session={s} />
            ))}
          </ul>
        )}
        {active > 0 && (
          <ConfirmButton
            labelId="remote-sessions-revoke-all"
            questionId="remote-sessions-revoke-all-confirm"
            questionArgs={{ count: active }}
            yesId="remote-sessions-revoke-all-yes"
            action="revoke-all"
            onConfirm={() =>
              run(() => client.request({ type: "RevokeAllRemoteSessions", user_id: null }, "Done"))
            }
          />
        )}
        <ErrorLine error={error} />
      </section>
    </section>
  );
}
