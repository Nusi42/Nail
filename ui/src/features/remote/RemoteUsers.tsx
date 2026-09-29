/**
 * Settings → Remote access → Users (layout widget `remote-users`): the
 * people who may use the remote UI, with what they may do (a preset plus
 * admin), their password, TOTP and passkeys, lockouts, and "pair a
 * device" for each. There are no default credentials: a new user has no
 * password and can only sign in from a device paired with a code.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";
import { errorMessageIdOf } from "../../api/client";
import type {
  Passkey,
  RemoteAccessPreset,
  RemoteUser,
  RemoteUserId,
  TotpEnrollment,
} from "../../api/generated";
import { useLabel, useLocale, useT } from "../../i18n/I18nProvider";
import type { WidgetProps } from "../../layout/widgetRegistry";
import { useClient } from "../../state/queries";
import { useUiStoreApi } from "../../state/uiStore";
import { SelectField } from "../../widgets/forms";
import { ConfirmButton, InputField, Toggle } from "./fields";
import { MIN_PASSWORD_LENGTH, PRESETS, extrasOf, formatTime, presetGrant, presetOf } from "./model";
import { PAIR_DIALOG } from "./PairDevice";
import { QrCode } from "./QrCode";
import { remoteKeys, useRemoteUsers } from "./queries";
import { remoteStore } from "./state";

type Notice = { id: string; error: boolean; args?: Record<string, string> } | null;

/** Runs a request and refreshes the remote access data; returns the notice. */
function useAccessRequest() {
  const client = useClient();
  const queryClient = useQueryClient();
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<Notice>(null);
  const run = async (fn: () => Promise<unknown>, ok: string | null = null): Promise<boolean> => {
    setBusy(true);
    setNotice(null);
    try {
      await fn();
      await queryClient.invalidateQueries({ queryKey: remoteKeys.all });
      if (ok !== null) setNotice({ id: ok, error: false });
      return true;
    } catch (e) {
      setNotice({ id: errorMessageIdOf(e), error: true });
      return false;
    } finally {
      setBusy(false);
    }
  };
  return { client, busy, notice, setNotice, run };
}

function NoticeLine({ notice }: { notice: Notice }) {
  const t = useT();
  if (notice === null) return null;
  return (
    <p
      data-part="remote-users.notice"
      data-state={notice.error ? "error" : "ok"}
      role={notice.error ? "alert" : "status"}
    >
      {t(notice.id, notice.args)}
    </p>
  );
}

function CreateUser({ onCreated }: { onCreated: (id: RemoteUserId) => void }) {
  const t = useT();
  const { client, busy, notice, run } = useAccessRequest();
  const [name, setName] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [preset, setPreset] = useState<RemoteAccessPreset>("no-send");
  const [admin, setAdmin] = useState(false);

  const create = async (event: FormEvent) => {
    event.preventDefault();
    if (busy || name.trim() === "") return;
    let id: RemoteUserId | null = null;
    const ok = await run(async () => {
      const answer = await client.request(
        {
          type: "CreateRemoteUser",
          name: name.trim(),
          display_name: displayName.trim() === "" ? null : displayName.trim(),
          grant: presetGrant(preset, { admin }),
        },
        "RemoteUserSaved",
      );
      id = answer.user.id;
    });
    if (ok && id !== null) {
      setName("");
      setDisplayName("");
      setAdmin(false);
      onCreated(id);
    }
  };

  return (
    <form
      data-part="remote-users.create"
      aria-label={t("remote-user-create-title")}
      noValidate
      onSubmit={(e) => void create(e)}
    >
      <h4 data-part="remote-users.subtitle">{t("remote-user-create-title")}</h4>
      <InputField labelId="remote-user-name" name="user-name" value={name} onChange={setName} />
      <InputField
        labelId="remote-user-display-name"
        name="display-name"
        value={displayName}
        onChange={setDisplayName}
      />
      <SelectField
        labelId="remote-access"
        name="user-access"
        value={preset}
        onChange={(v) => setPreset(v as RemoteAccessPreset)}
        choices={PRESETS.map((p) => ({ value: p, label: `remote-preset-${p}` }))}
      />
      <Toggle labelId="remote-access-admin" name="user-admin" checked={admin} onChange={setAdmin} />
      <button
        type="submit"
        data-part="form.button"
        data-variant="primary"
        data-action="create-user"
        disabled={busy || name.trim() === ""}
      >
        {t("remote-user-create")}
      </button>
      {notice && (
        <NoticeLine
          notice={
            notice.id === "error-conflict" ? { ...notice, id: "remote-user-name-taken" } : notice
          }
        />
      )}
    </form>
  );
}

function Access({ user }: { user: RemoteUser }) {
  const t = useT();
  const { client, busy, notice, run } = useAccessRequest();
  const current = presetOf(user.grant);
  const [preset, setPreset] = useState<RemoteAccessPreset | "custom">(current);
  const [admin, setAdmin] = useState(user.grant.admin);
  const dirty = preset !== current || admin !== user.grant.admin;

  const save = () =>
    void run(
      () =>
        client.request(
          {
            type: "UpdateRemoteUser",
            user_id: user.id,
            update: {
              display_name: null,
              disabled: null,
              grant:
                preset === "custom"
                  ? { ...user.grant, admin }
                  : presetGrant(preset, { admin, scopes: extrasOf(user.grant) }),
            },
          },
          "RemoteUserSaved",
        ),
      "remote-user-access-saved",
    );

  return (
    <fieldset data-part="form.section" data-section="access">
      <legend data-part="form.section.title">{t("remote-access")}</legend>
      <SelectField
        labelId="remote-access"
        name="access"
        value={preset}
        onChange={(v) => setPreset(v as RemoteAccessPreset | "custom")}
        choices={[
          ...PRESETS.map((p) => ({ value: p, label: `remote-preset-${p}` })),
          ...(current === "custom" ? [{ value: "custom", label: "remote-preset-custom" }] : []),
        ]}
      />
      <p data-part="form.hint">{t(`remote-preset-${preset}-hint`)}</p>
      <Toggle labelId="remote-access-admin" name="admin" checked={admin} onChange={setAdmin} />
      <p data-part="form.hint">{t("remote-access-narrowing")}</p>
      <button
        type="button"
        data-part="form.button"
        data-action="save-access"
        disabled={busy || !dirty}
        onClick={save}
      >
        {t("remote-user-access-save")}
      </button>
      <NoticeLine notice={notice} />
    </fieldset>
  );
}

function Password({ user }: { user: RemoteUser }) {
  const t = useT();
  const { client, busy, notice, setNotice, run } = useAccessRequest();
  const [password, setPassword] = useState("");
  const [repeat, setRepeat] = useState("");

  const save = async (event: FormEvent) => {
    event.preventDefault();
    if (busy) return;
    if ([...password].length < MIN_PASSWORD_LENGTH) {
      setNotice({
        id: "remote-password-too-short",
        error: true,
        args: { min: String(MIN_PASSWORD_LENGTH) },
      });
      return;
    }
    if (password !== repeat) {
      setNotice({ id: "remote-password-mismatch", error: true });
      return;
    }
    const ok = await run(
      () =>
        client.request(
          { type: "SetRemotePassword", user_id: user.id, password },
          "RemoteUserSaved",
        ),
      "remote-password-saved",
    );
    if (ok) {
      setPassword("");
      setRepeat("");
    }
  };

  return (
    <form
      data-part="form.section"
      data-section="password"
      aria-label={t("remote-password-title")}
      noValidate
      onSubmit={(e) => void save(e)}
    >
      <h4 data-part="remote-users.subtitle">{t("remote-password-title")}</h4>
      <p data-part="form.hint">
        {t(user.has_password ? "remote-password-set" : "remote-password-none")}
      </p>
      <InputField
        labelId={user.has_password ? "remote-password-new" : "remote-password"}
        name="new-password"
        type="password"
        autoComplete="new-password"
        value={password}
        onChange={setPassword}
        hint={t("remote-password-hint", { min: MIN_PASSWORD_LENGTH })}
      />
      <InputField
        labelId="remote-password-repeat"
        name="repeat-password"
        type="password"
        autoComplete="new-password"
        value={repeat}
        onChange={setRepeat}
      />
      <div data-part="remote-users.actions">
        <button
          type="submit"
          data-part="form.button"
          data-action="set-password"
          disabled={busy || password === ""}
        >
          {t("remote-password-save")}
        </button>
        {user.has_password && (
          <ConfirmButton
            labelId="remote-password-remove"
            questionId="remote-password-remove-confirm"
            questionArgs={{ user: user.name }}
            yesId="remote-password-remove-yes"
            action="remove-password"
            onConfirm={() =>
              run(
                () =>
                  client.request(
                    { type: "SetRemotePassword", user_id: user.id, password: null },
                    "RemoteUserSaved",
                  ),
                "remote-password-removed",
              )
            }
          />
        )}
      </div>
      <NoticeLine notice={notice} />
    </form>
  );
}

function Totp({ user }: { user: RemoteUser }) {
  const t = useT();
  const { client, busy, notice, setNotice, run } = useAccessRequest();
  const [enrollment, setEnrollment] = useState<TotpEnrollment | null>(null);
  const [code, setCode] = useState("");

  const start = () =>
    void run(async () => {
      const answer = await client.request(
        { type: "StartTotpEnrollment", user_id: user.id },
        "TotpEnrollment",
      );
      setEnrollment(answer.enrollment);
      setCode("");
    });

  const confirm = async (event: FormEvent) => {
    event.preventDefault();
    if (busy || code.trim() === "") return;
    const ok = await run(
      () =>
        client.request(
          { type: "ConfirmTotpEnrollment", user_id: user.id, code: code.replace(/\s/g, "") },
          "RemoteUserSaved",
        ),
      "remote-totp-enabled",
    );
    if (ok) setEnrollment(null);
    else {
      setNotice((n) =>
        n?.id === "error-auth-failed" ? { id: "remote-totp-wrong-code", error: true } : n,
      );
    }
  };

  return (
    <div data-part="form.section" data-section="totp">
      <h4 data-part="remote-users.subtitle">{t("remote-totp-title")}</h4>
      <p data-part="form.hint">{t(user.totp_enabled ? "remote-totp-on" : "remote-totp-off")}</p>
      {enrollment === null ? (
        <div data-part="remote-users.actions">
          <button
            type="button"
            data-part="form.button"
            data-action="start-totp"
            disabled={busy}
            onClick={start}
          >
            {t(user.totp_enabled ? "remote-totp-replace" : "remote-totp-start")}
          </button>
          {user.totp_enabled && (
            <ConfirmButton
              labelId="remote-totp-disable"
              questionId="remote-totp-disable-confirm"
              questionArgs={{ user: user.name }}
              yesId="remote-totp-disable-yes"
              action="disable-totp"
              onConfirm={() =>
                run(
                  () =>
                    client.request({ type: "DisableTotp", user_id: user.id }, "RemoteUserSaved"),
                  "remote-totp-disabled",
                )
              }
            />
          )}
        </div>
      ) : (
        <form
          data-part="remote-totp"
          aria-label={t("remote-totp-enroll-title")}
          noValidate
          onSubmit={(e) => void confirm(e)}
        >
          <p data-part="remote-totp.intro">{t("remote-totp-scan")}</p>
          <div data-part="remote-totp.qr">
            <QrCode data={enrollment.otpauth_uri} label={t("remote-totp-qr-label")} />
          </div>
          <p data-part="remote-totp.manual">
            {t("remote-totp-manual", {
              issuer: enrollment.issuer,
              account: enrollment.account_name,
            })}
          </p>
          <code data-part="remote-totp.secret" aria-label={t("remote-totp-secret")}>
            {enrollment.secret.match(/.{1,4}/g)?.join(" ") ?? enrollment.secret}
          </code>
          <p data-part="form.hint">{t("remote-totp-once")}</p>
          <InputField
            labelId="remote-totp-code"
            name="totp-code"
            value={code}
            onChange={setCode}
            inputMode="numeric"
            autoComplete="one-time-code"
            autoFocus
            maxLength={enrollment.digits + 2}
          />
          <div data-part="remote-users.actions">
            <button
              type="submit"
              data-part="form.button"
              data-variant="primary"
              data-action="confirm-totp"
              disabled={busy || code.trim() === ""}
            >
              {t("remote-totp-confirm")}
            </button>
            <button
              type="button"
              data-part="form.button"
              data-action="cancel"
              onClick={() => {
                setEnrollment(null);
                setNotice(null);
              }}
            >
              {t("remote-cancel")}
            </button>
          </div>
        </form>
      )}
      <NoticeLine notice={notice} />
    </div>
  );
}

function PasskeyRow({ passkey }: { passkey: Passkey }) {
  const t = useT();
  const locale = useLocale();
  const { client, busy, notice, run } = useAccessRequest();
  const [name, setName] = useState<string | null>(null);

  const rename = async (event: FormEvent) => {
    event.preventDefault();
    if (name === null || name.trim() === "" || busy) return;
    const ok = await run(() =>
      client.request(
        { type: "RenamePasskey", passkey_id: passkey.id, name: name.trim() },
        "RemoteUserSaved",
      ),
    );
    if (ok) setName(null);
  };

  return (
    <li data-part="remote-passkeys.item" data-passkey={passkey.id}>
      {name === null ? (
        <>
          <span data-part="remote-passkeys.name">{passkey.name}</span>
          <span data-part="remote-passkeys.meta">
            {t("remote-passkey-meta", {
              created: formatTime(passkey.created_at, locale),
              used:
                passkey.last_used_at === null
                  ? t("remote-never")
                  : formatTime(passkey.last_used_at, locale),
            })}
          </span>
          <button
            type="button"
            data-part="form.button"
            data-action="rename-passkey"
            aria-label={t("remote-passkey-rename-label", { name: passkey.name })}
            onClick={() => setName(passkey.name)}
          >
            {t("remote-rename")}
          </button>
          <ConfirmButton
            labelId="remote-passkey-delete"
            questionId="remote-passkey-delete-confirm"
            questionArgs={{ name: passkey.name }}
            yesId="remote-passkey-delete-yes"
            action="delete-passkey"
            onConfirm={() =>
              run(() =>
                client.request(
                  { type: "DeletePasskey", passkey_id: passkey.id },
                  "RemoteUserSaved",
                ),
              )
            }
          />
        </>
      ) : (
        <form data-part="remote-passkeys.rename" noValidate onSubmit={(e) => void rename(e)}>
          <InputField
            labelId="remote-passkey-new-name"
            name="passkey-name"
            value={name}
            onChange={setName}
            autoFocus
          />
          <button
            type="submit"
            data-part="form.button"
            data-action="save-name"
            disabled={busy || name.trim() === ""}
          >
            {t("remote-save")}
          </button>
          <button
            type="button"
            data-part="form.button"
            data-action="cancel"
            onClick={() => setName(null)}
          >
            {t("remote-cancel")}
          </button>
        </form>
      )}
      <NoticeLine notice={notice} />
    </li>
  );
}

function UserDetail({ user }: { user: RemoteUser }) {
  const t = useT();
  const locale = useLocale();
  const uiStore = useUiStoreApi();
  const { client, busy, notice, run } = useAccessRequest();
  const [now] = useState(Date.now);
  const locked = user.locked_until !== null && new Date(user.locked_until).getTime() > now;

  return (
    <section
      data-part="remote-users.detail"
      data-user={user.id}
      aria-label={t("remote-user-detail", { user: user.name })}
    >
      <h4 data-part="remote-users.subtitle">{user.display_name ?? user.name}</h4>
      <p data-part="remote-users.meta">
        {t("remote-user-meta", {
          name: user.name,
          created: formatTime(user.created_at, locale),
          login:
            user.last_login_at === null
              ? t("remote-never")
              : formatTime(user.last_login_at, locale),
        })}
      </p>
      {locked && (
        <div data-part="remote-users.locked" role="alert">
          <span data-part="remote-users.locked.text">
            {t("remote-user-locked", { until: formatTime(user.locked_until, locale) })}
          </span>
          <button
            type="button"
            data-part="form.button"
            data-action="unlock"
            disabled={busy}
            onClick={() =>
              void run(
                () =>
                  client.request({ type: "UnlockRemoteUser", user_id: user.id }, "RemoteUserSaved"),
                "remote-user-unlocked",
              )
            }
          >
            {t("remote-user-unlock")}
          </button>
        </div>
      )}
      <div data-part="remote-users.actions">
        <button
          type="button"
          data-part="form.button"
          data-variant="primary"
          data-action="pair-device"
          disabled={user.disabled}
          onClick={() => {
            remoteStore.getState().setPairUser(user.id);
            uiStore.getState().openWindow(PAIR_DIALOG);
          }}
        >
          {t("remote-pair-open-dialog")}
        </button>
      </div>
      <Access key={JSON.stringify(user.grant)} user={user} />
      <Password user={user} />
      <Totp user={user} />
      <div data-part="form.section" data-section="passkeys">
        <h4 data-part="remote-users.subtitle">{t("remote-passkeys-title")}</h4>
        {user.passkeys.length === 0 ? (
          <p data-part="form.hint">{t("remote-passkeys-none")}</p>
        ) : (
          <ul data-part="remote-passkeys" aria-label={t("remote-passkeys-title")}>
            {user.passkeys.map((p) => (
              <PasskeyRow key={p.id} passkey={p} />
            ))}
          </ul>
        )}
      </div>
      <div data-part="remote-users.danger">
        <button
          type="button"
          data-part="form.button"
          data-action={user.disabled ? "enable-user" : "disable-user"}
          disabled={busy}
          onClick={() =>
            void run(
              () =>
                client.request(
                  {
                    type: "UpdateRemoteUser",
                    user_id: user.id,
                    update: { display_name: null, grant: null, disabled: !user.disabled },
                  },
                  "RemoteUserSaved",
                ),
              user.disabled ? "remote-user-enabled" : "remote-user-disabled",
            )
          }
        >
          {t(user.disabled ? "remote-user-enable" : "remote-user-disable")}
        </button>
        <ConfirmButton
          labelId="remote-user-delete"
          questionId="remote-user-delete-confirm"
          questionArgs={{ user: user.name }}
          yesId="remote-user-delete-yes"
          action="delete-user"
          onConfirm={() =>
            run(() => client.request({ type: "DeleteRemoteUser", user_id: user.id }, "Done"))
          }
        />
      </div>
      <NoticeLine notice={notice} />
    </section>
  );
}

/** The `remote-users` widget. */
export function RemoteUsersWidget({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const users = useRemoteUsers();
  const [selected, setSelected] = useState<RemoteUserId | null>(null);
  const list = users.data ?? [];
  const current = list.find((u) => u.id === selected) ?? list[0];

  return (
    <section
      data-part="remote-users"
      data-node-id={node.id ?? undefined}
      aria-label={node.label ? label(node.label) : t("remote-users-title")}
    >
      <h3 data-part="remote-users.title">{t("remote-users-title")}</h3>
      <p data-part="remote-users.intro">{t("remote-users-intro")}</p>
      {users.isError && (
        <p data-part="remote-users.notice" data-state="error" role="alert">
          {t(errorMessageIdOf(users.error))}
        </p>
      )}
      {users.isSuccess && list.length === 0 && (
        <p data-part="remote-users.empty">{t("remote-users-empty")}</p>
      )}
      {list.length > 0 && (
        <ul data-part="remote-users.list" aria-label={t("remote-users-title")}>
          {list.map((u) => (
            <li key={u.id} data-part="remote-users.item">
              <button
                type="button"
                data-part="remote-users.select"
                data-state={`${u.disabled ? "disabled" : "active"}${u.id === current?.id ? " selected" : ""}`}
                aria-pressed={u.id === current?.id}
                onClick={() => setSelected(u.id)}
              >
                <span data-part="remote-users.name">{u.display_name ?? u.name}</span>
                <span data-part="remote-users.methods">
                  {t("remote-user-methods", {
                    passkeys: u.passkeys.length,
                    password: u.has_password ? "yes" : "no",
                    totp: u.totp_enabled ? "yes" : "no",
                  })}
                </span>
                {u.disabled && (
                  <span data-part="remote-users.badge" data-state="disabled">
                    {t("remote-user-disabled-badge")}
                  </span>
                )}
              </button>
            </li>
          ))}
        </ul>
      )}
      {current !== undefined && <UserDetail key={current.id} user={current} />}
      <CreateUser onCreated={setSelected} />
    </section>
  );
}
