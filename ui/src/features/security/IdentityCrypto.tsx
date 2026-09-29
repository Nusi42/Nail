/**
 * Per-identity end-to-end settings (`Identity.crypto`, the identity's
 * `crypto` node in `accounts.kdl`, saved with `UpdateAccount`).
 *
 * `IdentityCryptoFields` is a controlled editor the account settings can
 * mount inside each identity; the layout widget `security-identities`
 * edits every identity of every account on its own.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { errorMessageIdOf } from "../../api/client";
import type {
  AccountConfig,
  CryptoKey,
  CryptoPreference,
  Identity,
  IdentityCrypto,
} from "../../api/generated";
import { useLabel, useT } from "../../i18n/I18nProvider";
import type { WidgetProps } from "../../layout/widgetRegistry";
import { queryKeys, useAccountsConfig, useClient } from "../../state/queries";
import { CheckField, SelectField } from "../../widgets/forms";
import { PREFERENCES, defaultIdentityCrypto, formatFingerprint, ownKeysFor } from "./model";
import { useOwnKeys } from "./queries";

/** `Ada Lovelace <ada@example.org>`. */
export function identityText(identity: Pick<Identity, "name" | "email">): string {
  return identity.name ? `${identity.name} <${identity.email}>` : identity.email;
}

function keyChoice(key: CryptoKey) {
  return {
    value: key.fingerprint,
    label: `${key.user_ids[0] ?? ""} (${formatFingerprint(key.fingerprint.slice(-16))})`,
    literal: true,
  };
}

/** Controlled editor of one identity's crypto settings. */
export function IdentityCryptoFields({
  identity,
  ownKeys,
  onChange,
}: {
  identity: Identity;
  /** The keyring's own keys (`ListKeys own_only`); filtered by the identity's address here. */
  ownKeys: readonly CryptoKey[];
  onChange: (crypto: IdentityCrypto) => void;
}) {
  const t = useT();
  const crypto = identity.crypto ?? defaultIdentityCrypto();
  const set = (patch: Partial<IdentityCrypto>) => onChange({ ...crypto, ...patch });
  const pgpKeys = ownKeysFor(ownKeys, "openpgp", identity.email);
  const smimeCerts = ownKeysFor(ownKeys, "smime", identity.email);
  const name = identity.id || identity.email;
  // A configured key that is gone (deleted, expired) stays visible as a choice.
  const missing = (fp: string | null, list: CryptoKey[]) =>
    fp !== null && !list.some((k) => k.fingerprint === fp)
      ? [{ value: fp, label: formatFingerprint(fp), literal: true }]
      : [];

  return (
    <fieldset
      data-part="security-identity"
      data-identity={identity.id}
      aria-label={t("security-identity-label", { identity: identityText(identity) })}
    >
      <legend data-part="security-identity.title">{identityText(identity)}</legend>
      <SelectField
        labelId="security-identity-technology"
        name={`${name}-technology`}
        value={crypto.technology}
        onChange={(v) => set({ technology: v as CryptoPreference })}
        choices={PREFERENCES.map((p) => ({ value: p, label: `security-preference-${p}` }))}
      />
      <CheckField
        labelId="security-identity-sign"
        name={`${name}-sign`}
        checked={crypto.sign}
        onChange={(sign) => set({ sign })}
      />
      <CheckField
        labelId="security-identity-encrypt"
        name={`${name}-encrypt`}
        checked={crypto.encrypt}
        onChange={(encrypt) => set({ encrypt })}
      />
      <p data-part="form.hint">{t("security-identity-encrypt-hint")}</p>
      <SelectField
        labelId="security-identity-openpgp-key"
        name={`${name}-openpgp-key`}
        value={crypto.openpgp_key ?? ""}
        onChange={(v) => set({ openpgp_key: v === "" ? null : v })}
        choices={[
          { value: "", label: "security-identity-key-auto" },
          ...pgpKeys.map(keyChoice),
          ...missing(crypto.openpgp_key, pgpKeys),
        ]}
      />
      {pgpKeys.length === 0 && (
        <p data-part="security-identity.no-key" data-technology="openpgp" role="note">
          {t("security-identity-no-openpgp-key")}
        </p>
      )}
      <SelectField
        labelId="security-identity-smime-cert"
        name={`${name}-smime-cert`}
        value={crypto.smime_certificate ?? ""}
        onChange={(v) => set({ smime_certificate: v === "" ? null : v })}
        choices={[
          { value: "", label: "security-identity-key-auto" },
          ...smimeCerts.map(keyChoice),
          ...missing(crypto.smime_certificate, smimeCerts),
        ]}
      />
      {smimeCerts.length === 0 && (
        <p data-part="security-identity.no-key" data-technology="smime" role="note">
          {t("security-identity-no-smime-cert")}
        </p>
      )}
      <CheckField
        labelId="security-identity-autocrypt"
        name={`${name}-autocrypt`}
        checked={crypto.autocrypt}
        onChange={(autocrypt) => set({ autocrypt })}
      />
      {crypto.autocrypt && (
        <CheckField
          labelId="security-identity-prefer-encrypt"
          name={`${name}-prefer-encrypt`}
          checked={crypto.prefer_encrypt === "mutual"}
          onChange={(on) => set({ prefer_encrypt: on ? "mutual" : "nopreference" })}
        />
      )}
      <CheckField
        labelId="security-identity-protect-headers"
        name={`${name}-protect-headers`}
        checked={crypto.protect_headers}
        onChange={(protect_headers) => set({ protect_headers })}
      />
      <p data-part="form.hint">{t("security-identity-protect-headers-hint")}</p>
    </fieldset>
  );
}

function AccountCrypto({ account, ownKeys }: { account: AccountConfig; ownKeys: CryptoKey[] }) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const [draft, setDraft] = useState<AccountConfig>(account);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<{ id: string; error: boolean } | null>(null);
  const dirty = JSON.stringify(draft.identities) !== JSON.stringify(account.identities);

  const save = async () => {
    setBusy(true);
    setMessage(null);
    try {
      // Only the identities' crypto changes; everything else is sent as loaded.
      const { config } = await client.request(
        { type: "UpdateAccount", config: draft, secrets: [] },
        "AccountSaved",
      );
      setDraft(config);
      setMessage({ id: "security-identities-saved", error: false });
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: queryKeys.accountsConfig }),
        queryClient.invalidateQueries({ queryKey: queryKeys.allIdentities }),
      ]);
    } catch (e) {
      setMessage({ id: errorMessageIdOf(e), error: true });
    } finally {
      setBusy(false);
    }
  };

  return (
    <section
      data-part="security-identities.account"
      data-account={account.id}
      aria-label={account.name}
    >
      <h3 data-part="security-identities.account.name">{account.name}</h3>
      {draft.identities.map((identity, index) => (
        <IdentityCryptoFields
          key={identity.id || index}
          identity={identity}
          ownKeys={ownKeys}
          onChange={(crypto) =>
            setDraft({
              ...draft,
              identities: draft.identities.map((old, i) =>
                i === index ? { ...old, crypto } : old,
              ),
            })
          }
        />
      ))}
      <button
        type="button"
        data-part="form.button"
        data-variant="primary"
        data-action="save-identities"
        disabled={busy || !dirty}
        onClick={() => void save()}
      >
        {t("security-identities-save", { account: account.name })}
      </button>
      {message && (
        <p
          data-part="security-identities.message"
          data-state={message.error ? "error" : "ok"}
          role={message.error ? "alert" : "status"}
        >
          {t(message.id)}
        </p>
      )}
    </section>
  );
}

/** Layout widget `security-identities`: crypto settings of every identity. */
export function IdentityCryptoSettings({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const config = useAccountsConfig();
  const keys = useOwnKeys();
  const accounts = config.data?.accounts ?? [];
  return (
    <section
      data-part="security-identities"
      data-node-id={node.id ?? undefined}
      aria-label={node.label ? label(node.label) : t("security-identities-label")}
    >
      <p data-part="security-identities.intro">{t("security-identities-intro")}</p>
      {config.isError && (
        <p data-part="security-identities.message" data-state="error" role="alert">
          {t(errorMessageIdOf(config.error))}
        </p>
      )}
      {config.isSuccess && accounts.length === 0 && (
        <p data-part="security-identities.empty">{t("security-identities-empty")}</p>
      )}
      {accounts.map((account) => (
        <AccountCrypto
          key={`${account.id}:${JSON.stringify(account.identities.map((i) => i.crypto ?? null))}`}
          account={account}
          ownKeys={keys.data ?? []}
        />
      ))}
    </section>
  );
}
