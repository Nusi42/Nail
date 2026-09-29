/**
 * Pairing, desktop side (dialog `remote-pair`, action `remote.pair`):
 * choose the user and what the new device may do, then show the one-time
 * code with the address to open and the certificate fingerprint the
 * browser must show too. The dialog follows the code until a browser
 * uses it (`RemoteAccessChanged` refreshes the list).
 */
import { useQueryClient } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";
import { errorMessageIdOf } from "../../api/client";
import type {
  PairingCode,
  RemoteAccessPreset,
  RemoteUser,
  RemoteUserId,
} from "../../api/generated";
import { useLocale, useT } from "../../i18n/I18nProvider";
import type { DialogProps } from "../../layout/dialogRegistry";
import { useClient } from "../../state/queries";
import { SelectField } from "../../widgets/forms";
import { InputField, Toggle } from "./fields";
import {
  PAIRING_TTLS,
  PRESETS,
  formatPairingCode,
  formatTime,
  groupFingerprint,
  presetGrant,
} from "./model";
import { QrCode } from "./QrCode";
import { remoteKeys, usePairingCodes, useRemoteUsers } from "./queries";
import { useRemoteState } from "./state";

/** The dialog's name (`window.open "remote-pair"`). */
export const PAIR_DIALOG = "remote-pair";

interface Created {
  pairing: PairingCode;
  code: string;
  url: string | null;
  fingerprint: string | null;
}

function CodePanel({ created, onClose }: { created: Created; onClose: () => void }) {
  const t = useT();
  const locale = useLocale();
  const client = useClient();
  const queryClient = useQueryClient();
  const codes = usePairingCodes();
  const [error, setError] = useState<string | null>(null);
  const current = codes.data?.find((c) => c.id === created.pairing.id) ?? created.pairing;

  const cancel = async () => {
    setError(null);
    try {
      await client.request({ type: "CancelPairingCode", pairing_id: current.id }, "Done");
      await queryClient.invalidateQueries({ queryKey: remoteKeys.access });
    } catch (e) {
      setError(errorMessageIdOf(e));
    }
  };

  return (
    <section
      data-part="remote-pair.code-panel"
      data-state={current.state}
      aria-label={t("remote-pair-code-title")}
    >
      <p data-part="remote-pair.state" data-state={current.state} role="status">
        {t(`remote-pairing-state-${current.state}`)}
      </p>
      {current.state === "pending" && (
        <>
          <p data-part="remote-pair.instructions">
            {t(created.url === null ? "remote-pair-open-not-running" : "remote-pair-open", {
              url: created.url ?? "",
            })}
          </p>
          <p data-part="remote-pair.code-label">{t("remote-pair-code-title")}</p>
          <code data-part="remote-pair.code" aria-label={t("remote-pair-code-title")}>
            {formatPairingCode(created.code)}
          </code>
          <p data-part="form.hint">
            {t("remote-pair-expires", { time: formatTime(current.expires_at, locale) })}
          </p>
          {created.url !== null && (
            <div data-part="remote-pair.qr">
              <QrCode data={created.url} label={t("remote-pair-qr-label", { url: created.url })} />
            </div>
          )}
          <div data-part="remote-pair.fingerprint">
            {created.fingerprint !== null ? (
              <>
                <p data-part="remote-pair.fingerprint.intro">
                  {t("remote-pair-fingerprint-intro")}
                </p>
                <code data-part="remote-fingerprint">{groupFingerprint(created.fingerprint)}</code>
              </>
            ) : (
              <p data-part="remote-pair.fingerprint.intro">{t("remote-pair-no-fingerprint")}</p>
            )}
          </div>
          <p data-part="form.hint">{t("remote-pair-once")}</p>
        </>
      )}
      {error && (
        <p data-part="remote-pair.error" role="alert">
          {t(error)}
        </p>
      )}
      <div data-part="remote-pair.actions">
        {current.state === "pending" && (
          <button
            type="button"
            data-part="form.button"
            data-action="cancel-code"
            data-variant="danger"
            onClick={() => void cancel()}
          >
            {t("remote-pair-cancel-code")}
          </button>
        )}
        <button
          type="button"
          data-part="form.button"
          data-action="close"
          data-variant={current.state === "pending" ? undefined : "primary"}
          onClick={onClose}
        >
          {t("remote-pair-close")}
        </button>
      </div>
    </section>
  );
}

function CreateForm({
  users,
  initialUser,
  onCreated,
}: {
  users: readonly RemoteUser[];
  initialUser: RemoteUserId | null;
  onCreated: (created: Created) => void;
}) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const usable = users.filter((u) => !u.disabled);
  const [userId, setUserId] = useState<RemoteUserId>(
    usable.find((u) => u.id === initialUser)?.id ?? usable[0]?.id ?? "",
  );
  const [deviceName, setDeviceName] = useState("");
  const [preset, setPreset] = useState<RemoteAccessPreset>("no-send");
  const [admin, setAdmin] = useState(false);
  const [ttl, setTtl] = useState(10);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // The chosen user, else the first one (the list may arrive after mounting).
  const user = usable.find((u) => u.id === userId) ?? usable[0];

  if (usable.length === 0) {
    return (
      <p data-part="remote-pair.empty" role="note">
        {t("remote-pair-no-users")}
      </p>
    );
  }

  const create = async (event: FormEvent) => {
    event.preventDefault();
    if (busy || user === undefined) return;
    setBusy(true);
    setError(null);
    try {
      const answer = await client.request(
        {
          type: "CreatePairingCode",
          user_id: user.id,
          device_name: deviceName.trim() === "" ? null : deviceName.trim(),
          grant: presetGrant(preset, { admin: admin && user.grant.admin }),
          ttl_minutes: ttl,
        },
        "PairingCodeCreated",
      );
      await queryClient.invalidateQueries({ queryKey: remoteKeys.access });
      onCreated({
        pairing: answer.pairing,
        code: answer.code,
        url: answer.url,
        fingerprint: answer.certificate_fingerprint,
      });
    } catch (e) {
      setError(errorMessageIdOf(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form data-part="remote-pair.form" noValidate onSubmit={(e) => void create(e)}>
      <p data-part="remote-pair.intro">{t("remote-pair-intro")}</p>
      <SelectField
        labelId="remote-pair-user"
        name="pair-user"
        value={user?.id ?? ""}
        onChange={setUserId}
        choices={usable.map((u) => ({
          value: u.id,
          label: u.display_name ? `${u.display_name} (${u.name})` : u.name,
          literal: true,
        }))}
      />
      <InputField
        labelId="remote-pair-device-name"
        hintId="remote-pair-device-name-desktop-hint"
        name="pair-device-name"
        value={deviceName}
        onChange={setDeviceName}
        maxLength={64}
      />
      <SelectField
        labelId="remote-access"
        name="pair-access"
        value={preset}
        onChange={(v) => setPreset(v as RemoteAccessPreset)}
        choices={PRESETS.map((p) => ({ value: p, label: `remote-preset-${p}` }))}
      />
      <p data-part="form.hint">{t(`remote-preset-${preset}-hint`)}</p>
      <Toggle
        labelId="remote-access-admin"
        name="pair-admin"
        checked={admin && (user?.grant.admin ?? false)}
        onChange={setAdmin}
        lockedReason={user?.grant.admin ? null : t("remote-access-admin-not-allowed")}
      />
      <SelectField
        labelId="remote-pair-ttl"
        name="pair-ttl"
        value={String(ttl)}
        onChange={(v) => setTtl(Number(v))}
        choices={PAIRING_TTLS.map((m) => ({
          value: String(m),
          label: t("remote-pair-ttl-minutes", { minutes: m }),
          literal: true,
        }))}
      />
      {error && (
        <p data-part="remote-pair.error" role="alert">
          {t(error)}
        </p>
      )}
      <div data-part="remote-pair.actions">
        <button
          type="submit"
          data-part="form.button"
          data-variant="primary"
          data-action="create-code"
          disabled={busy || user === undefined}
        >
          {t("remote-pair-create")}
        </button>
      </div>
    </form>
  );
}

/** The `remote-pair` dialog. */
export function PairDeviceDialog({ close }: DialogProps) {
  const t = useT();
  const users = useRemoteUsers();
  const initialUser = useRemoteState((s) => s.pairUser);
  const [created, setCreated] = useState<Created | null>(null);
  return (
    <section data-part="remote-pair" aria-label={t("window-remote-pair")}>
      {users.isError && (
        <p data-part="remote-pair.error" role="alert">
          {t(errorMessageIdOf(users.error))}
        </p>
      )}
      {created !== null ? (
        <CodePanel created={created} onClose={close} />
      ) : (
        users.data && (
          <CreateForm users={users.data} initialUser={initialUser} onCreated={setCreated} />
        )
      )}
    </section>
  );
}
