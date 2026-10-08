/**
 * Settings → Security (layout widget `security-settings`): encryption at
 * rest with migration progress and the "recommended: on" note, online key
 * lookup, and the opt-in to index decrypted mail per account.
 *
 * Encryption at rest ships off until the maintainer decides the default (plan
 * section 7, scope decisions); the UI recommends turning it on.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { errorMessageId, errorMessageIdOf } from "../../api/client";
import type {
  KeyLookupPolicy,
  KeyLookupSource,
  SecuritySettings as Settings,
  VaultStatus,
} from "../../api/generated";
import { useLabel, useT } from "../../i18n/I18nProvider";
import type { WidgetProps } from "../../layout/widgetRegistry";
import { useAccounts, useClient } from "../../state/queries";
import { CheckField, SelectField } from "../../widgets/forms";
import { applyVaultStatus, securityKeys, useSecuritySettings, useVaultStatus } from "./queries";

export const LOOKUP_POLICIES: readonly KeyLookupPolicy[] = ["off", "on-demand", "automatic"];
export const LOOKUP_SOURCES: readonly KeyLookupSource[] = ["wkd", "keys-openpgp-org"];

/** Share of the whole migration done (phases weigh equally), 0–100. */
export function vaultPercent(status: VaultStatus): number {
  if (status.phase === null)
    return status.state === "encrypted" || status.state === "plain" ? 100 : 0;
  const index = ["database", "blobs", "index"].indexOf(status.phase);
  const inPhase = status.total > 0 ? status.done / status.total : 0;
  return Math.round(((index + inPhase) / 3) * 100);
}

function Vault() {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const vault = useVaultStatus();
  const [confirm, setConfirm] = useState<boolean | null>(null);
  const [error, setError] = useState<string | null>(null);
  const status = vault.data;

  const set = async (enabled: boolean) => {
    setError(null);
    setConfirm(null);
    try {
      const answer = await client.request({ type: "SetVaultEncryption", enabled }, "VaultStatus");
      applyVaultStatus(queryClient, answer.status);
    } catch (e) {
      setError(errorMessageIdOf(e));
    }
  };

  if (vault.isError) {
    return (
      <p data-part="security-vault.error" role="alert">
        {t(errorMessageIdOf(vault.error))}
      </p>
    );
  }
  if (!status) return null;
  const migrating = status.state === "encrypting" || status.state === "decrypting";
  const on = status.state === "encrypted" || status.state === "encrypting";
  const percent = vaultPercent(status);

  return (
    <section
      data-part="security-vault"
      data-state={status.state}
      aria-label={t("security-vault-title")}
    >
      <h3 data-part="security-vault.title">{t("security-vault-title")}</h3>
      <p data-part="security-vault.intro">{t("security-vault-intro")}</p>
      <p data-part="security-vault.state" data-state={status.state} role="status">
        {t(`security-vault-state-${status.state}`)}
      </p>
      {status.state === "plain" && !migrating && (
        <p data-part="security-vault.recommendation" role="note">
          {t("security-vault-recommended")}
        </p>
      )}
      {status.key_in_keychain && (
        <p data-part="security-vault.key">{t("security-vault-key-keychain")}</p>
      )}
      {migrating && (
        <div data-part="security-vault.progress">
          <progress
            data-part="security-vault.bar"
            max={100}
            value={percent}
            aria-label={t("common-progress")}
          />
          <span data-part="security-vault.phase">
            {status.phase
              ? t("security-vault-phase", {
                  phase: t(`security-vault-phase-${status.phase}`),
                  done: status.done,
                  total: status.total,
                  percent,
                })
              : null}
          </span>
          <p data-part="form.hint">{t("security-vault-keep-open")}</p>
        </div>
      )}
      {status.error !== null && (
        <div data-part="security-vault.failed" role="alert">
          <p data-part="security-vault.failed.text">
            {t("security-vault-failed", { error: t(errorMessageId(status.error)) })}
          </p>
          <button
            type="button"
            data-part="form.button"
            data-action="vault-retry"
            onClick={() => void set(status.state === "encrypting")}
          >
            {t("common-try-again")}
          </button>
        </div>
      )}
      {!migrating && confirm === null && (
        <button
          type="button"
          data-part="form.button"
          data-variant={on ? undefined : "primary"}
          data-action={on ? "vault-off" : "vault-on"}
          onClick={() => {
            setConfirm(!on);
          }}
        >
          {t(on ? "security-vault-turn-off" : "security-vault-turn-on")}
        </button>
      )}
      {confirm !== null && (
        <div data-part="security-vault.confirm" role="group" aria-label={t("security-vault-title")}>
          <p data-part="security-vault.confirm.text">
            {t(confirm ? "security-vault-confirm-on" : "security-vault-confirm-off")}
          </p>
          <button
            type="button"
            data-part="form.button"
            data-variant={confirm ? "primary" : "danger"}
            data-action="vault-confirm"
            onClick={() => void set(confirm)}
          >
            {t(confirm ? "security-vault-confirm-on-yes" : "security-vault-confirm-off-yes")}
          </button>
          <button
            type="button"
            data-part="form.button"
            data-action="cancel"
            onClick={() => {
              setConfirm(null);
            }}
          >
            {t("security-cancel")}
          </button>
        </div>
      )}
      {error && (
        <p data-part="security-vault.error" role="alert">
          {t(error)}
        </p>
      )}
    </section>
  );
}

function LookupAndIndex({ settings }: { settings: Settings }) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const accounts = (useAccounts().data ?? []).filter((a) => a.kind !== "virtual");
  const [error, setError] = useState<string | null>(null);

  const save = async (next: Settings) => {
    setError(null);
    queryClient.setQueryData(securityKeys.settings, next);
    try {
      const answer = await client.request(
        { type: "UpdateSecuritySettings", settings: next },
        "SecuritySettings",
      );
      queryClient.setQueryData(securityKeys.settings, answer.settings);
    } catch (e) {
      setError(errorMessageIdOf(e));
      await queryClient.invalidateQueries({ queryKey: securityKeys.settings });
    }
  };

  return (
    <>
      <section data-part="security-lookup-settings" aria-label={t("security-lookup-title")}>
        <h3 data-part="security-settings.title">{t("security-lookup-title")}</h3>
        <SelectField
          labelId="security-lookup-policy"
          name="lookup-policy"
          value={settings.key_lookup}
          onChange={(v) => void save({ ...settings, key_lookup: v as KeyLookupPolicy })}
          choices={LOOKUP_POLICIES.map((p) => ({ value: p, label: `security-lookup-policy-${p}` }))}
        />
        <p data-part="form.hint">{t(`security-lookup-policy-${settings.key_lookup}-hint`)}</p>
        {settings.key_lookup !== "off" &&
          LOOKUP_SOURCES.map((source) => (
            <CheckField
              key={source}
              labelId={`security-lookup-source-${source}`}
              name={`lookup-source-${source}`}
              checked={settings.lookup_sources.includes(source)}
              onChange={(on) =>
                void save({
                  ...settings,
                  lookup_sources: LOOKUP_SOURCES.filter((s) =>
                    s === source ? on : settings.lookup_sources.includes(s),
                  ),
                })
              }
            />
          ))}
      </section>
      <section data-part="security-index-settings" aria-label={t("security-index-title")}>
        <h3 data-part="security-settings.title">{t("security-index-title")}</h3>
        <p data-part="security-settings.intro">{t("security-index-intro")}</p>
        {accounts.map((account) => (
          <CheckField
            key={account.id}
            labelId="security-index-account"
            args={{ account: account.name, email: account.email }}
            name={`index-${account.id}`}
            checked={settings.index_decrypted.includes(account.id)}
            onChange={(on) =>
              void save({
                ...settings,
                index_decrypted: on
                  ? [...settings.index_decrypted, account.id]
                  : settings.index_decrypted.filter((id) => id !== account.id),
              })
            }
          />
        ))}
      </section>
      {error && (
        <p data-part="security-settings.error" role="alert">
          {t(error)}
        </p>
      )}
    </>
  );
}

/** The `security-settings` widget. */
export function SecuritySettingsWidget({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const settings = useSecuritySettings();
  const vault = useVaultStatus().data;
  const indexing = (settings.data?.index_decrypted.length ?? 0) > 0;
  return (
    <section
      data-part="security-settings"
      data-node-id={node.id ?? undefined}
      aria-label={node.label ? label(node.label) : t("security-settings-label")}
    >
      <Vault />
      {settings.isError && (
        <p data-part="security-settings.error" role="alert">
          {t(errorMessageIdOf(settings.error))}
        </p>
      )}
      {settings.data && <LookupAndIndex settings={settings.data} />}
      {indexing && vault?.state === "plain" && (
        <p data-part="security-settings.warning" role="note">
          {t("security-index-plain-warning")}
        </p>
      )}
    </section>
  );
}
