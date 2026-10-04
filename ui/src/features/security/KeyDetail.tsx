/**
 * One key or certificate in the key manager: identity, fingerprint,
 * validity, capabilities, trust editing, the S/MIME chain, export,
 * unlocking and deletion.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import { errorMessageIdOf } from "../../api/client";
import type { CryptoKey, KeyEncoding, Response, TrustDecision } from "../../api/generated";
import { useLocale, useT } from "../../i18n/I18nProvider";
import { useClient } from "../../state/queries";
import { CheckField, SelectField } from "../../widgets/forms";
import { ChainView, UnlockForm } from "./MessageSecurity";
import {
  TRUST_DECISIONS,
  decisionOf,
  exportHref,
  formatDateTime,
  formatFingerprint,
  keyState,
} from "./model";
import { securityKeys } from "./queries";

type Exported = Extract<Response, { type: "KeyExported" }>;

function Field({ labelId, children }: { labelId: string; children: ReactNode }) {
  const t = useT();
  return (
    <div data-part="security-key.row">
      <dt data-part="security-key.label">{t(labelId)}</dt>
      <dd data-part="security-key.value">{children}</dd>
    </div>
  );
}

function TrustEditor({ keyInfo }: { keyInfo: CryptoKey }) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const current = decisionOf(keyInfo.trust);
  const [decision, setDecision] = useState<TrustDecision>(current);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const save = async () => {
    setError(null);
    setSaved(false);
    try {
      await client.request({ type: "SetKeyTrust", key_id: keyInfo.id, decision }, "KeySaved");
      setSaved(true);
      await queryClient.invalidateQueries({ queryKey: securityKeys.all });
    } catch (e) {
      setError(errorMessageIdOf(e));
    }
  };
  if (keyInfo.has_secret) return null;
  return (
    <div data-part="security-key.trust-editor">
      <SelectField
        labelId="security-key-trust-decision"
        name="trust-decision"
        value={decision}
        onChange={(v) => {
          setDecision(v as TrustDecision);
          setSaved(false);
        }}
        choices={TRUST_DECISIONS.map((d) => ({ value: d, label: `security-decision-${d}` }))}
      />
      <p data-part="form.hint">{t(`security-decision-${decision}-hint`)}</p>
      <button
        type="button"
        data-part="form.button"
        data-action="save-trust"
        disabled={decision === current}
        onClick={() => void save()}
      >
        {t("security-key-trust-save")}
      </button>
      {saved && (
        <p data-part="security-key.saved" role="status">
          {t("security-key-trust-saved")}
        </p>
      )}
      {error && (
        <p data-part="security-key.error" role="alert">
          {t(error)}
        </p>
      )}
    </div>
  );
}

function ExportPanel({ keyInfo }: { keyInfo: CryptoKey }) {
  const t = useT();
  const client = useClient();
  const [exported, setExported] = useState<Exported | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState<boolean | null>(null);
  const run = async (encoding: KeyEncoding) => {
    setError(null);
    setCopied(null);
    try {
      setExported(
        await client.request(
          { type: "ExportPublicKey", key_id: keyInfo.id, encoding },
          "KeyExported",
        ),
      );
    } catch (e) {
      setError(errorMessageIdOf(e));
    }
  };
  const copy = async (text: string) => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
    } catch {
      setCopied(false);
    }
  };
  return (
    <div data-part="security-key.export">
      <button
        type="button"
        data-part="form.button"
        data-action="export"
        onClick={() => void run("text")}
      >
        {t(
          keyInfo.technology === "openpgp"
            ? "security-key-export-openpgp"
            : "security-key-export-smime",
        )}
      </button>
      {exported && (
        <div
          data-part="security-key.exported"
          role="region"
          aria-label={t("security-key-exported-label")}
        >
          {exported.encoding === "text" && (
            <textarea
              data-part="security-key.exported.text"
              readOnly
              value={exported.data}
              aria-label={t("security-key-exported-label")}
            />
          )}
          <a
            data-part="form.button"
            data-action="download"
            href={exportHref(exported.mime, exported.encoding, exported.data)}
            download={exported.filename}
          >
            {t("security-key-download", { filename: exported.filename })}
          </a>
          {exported.encoding === "text" && (
            <button
              type="button"
              data-part="form.button"
              data-action="copy"
              onClick={() => void copy(exported.data)}
            >
              {t(copied ? "security-key-copied" : "security-key-copy")}
            </button>
          )}
          {copied === false && (
            <p data-part="security-key.error" role="alert">
              {t("security-key-copy-failed")}
            </p>
          )}
        </div>
      )}
      {error && (
        <p data-part="security-key.error" role="alert">
          {t(error)}
        </p>
      )}
    </div>
  );
}

function DeletePanel({ keyInfo, onDeleted }: { keyInfo: CryptoKey; onDeleted: () => void }) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const [confirm, setConfirm] = useState(false);
  const [withSecret, setWithSecret] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const own = keyInfo.has_secret;
  const remove = async () => {
    setError(null);
    try {
      await client.request(
        { type: "DeleteKey", key_id: keyInfo.id, delete_secret: own && withSecret },
        "Done",
      );
      await queryClient.invalidateQueries({ queryKey: securityKeys.all });
      onDeleted();
    } catch (e) {
      setError(errorMessageIdOf(e));
    }
  };
  if (!confirm) {
    return (
      <button
        type="button"
        data-part="form.button"
        data-variant="danger"
        data-action="delete"
        onClick={() => {
          setConfirm(true);
        }}
      >
        {t("security-key-delete")}
      </button>
    );
  }
  return (
    <div data-part="security-key.confirm" role="group" aria-label={t("security-key-delete")}>
      <p data-part="security-key.confirm.text">
        {t(own ? "security-key-delete-own-confirm" : "security-key-delete-confirm", {
          name: keyInfo.user_ids[0] ?? keyInfo.fingerprint,
        })}
      </p>
      {own && (
        <CheckField
          labelId="security-key-delete-secret"
          name="delete-secret"
          checked={withSecret}
          onChange={setWithSecret}
        />
      )}
      <button
        type="button"
        data-part="form.button"
        data-variant="danger"
        data-action="delete-confirm"
        disabled={own && !withSecret}
        onClick={() => void remove()}
      >
        {t("security-key-delete-yes")}
      </button>
      <button
        type="button"
        data-part="form.button"
        data-action="cancel"
        onClick={() => {
          setConfirm(false);
          setWithSecret(false);
        }}
      >
        {t("security-cancel")}
      </button>
      {error && (
        <p data-part="security-key.error" role="alert">
          {t(error)}
        </p>
      )}
    </div>
  );
}

/** Details and actions of one keyring entry. */
export function KeyDetail({ keyInfo, onDeleted }: { keyInfo: CryptoKey; onDeleted: () => void }) {
  const t = useT();
  const locale = useLocale();
  const state = keyState(keyInfo);
  const [unlocked, setUnlocked] = useState(false);
  return (
    <section
      data-part="security-key"
      data-key={keyInfo.id}
      data-technology={keyInfo.technology}
      data-state={state}
      aria-label={t("security-key-detail-label", { name: keyInfo.user_ids[0] ?? "" })}
    >
      <h3 data-part="security-key.title">
        {keyInfo.user_ids[0] ?? formatFingerprint(keyInfo.fingerprint)}
      </h3>
      {state !== "ok" && (
        <p data-part="security-key.state" data-state={state} role="note">
          {t(`security-key-state-${state}`)}
        </p>
      )}
      <dl data-part="security-key.list">
        <Field labelId="security-key-technology">
          {t(`security-technology-${keyInfo.technology}`)}
        </Field>
        {keyInfo.user_ids.length > 1 && (
          <Field labelId="security-key-user-ids">
            <ul data-part="security-key.user-ids">
              {keyInfo.user_ids.map((u) => (
                <li key={u} data-part="security-key.user-id">
                  {u}
                </li>
              ))}
            </ul>
          </Field>
        )}
        <Field labelId="security-key-addresses">{keyInfo.emails.join(", ")}</Field>
        <Field labelId="security-key-fingerprint">
          <code data-part="security-fingerprint">{formatFingerprint(keyInfo.fingerprint)}</code>
        </Field>
        <Field labelId="security-key-algorithm">{keyInfo.algorithm}</Field>
        <Field labelId="security-key-created">
          {formatDateTime(keyInfo.created_at, locale, false)}
        </Field>
        <Field labelId="security-key-expires">
          {keyInfo.expires_at === null
            ? t("security-key-never-expires")
            : formatDateTime(keyInfo.expires_at, locale, false)}
        </Field>
        <Field labelId="security-key-usage">
          {t("security-key-usage-value", {
            sign: keyInfo.can_sign ? "yes" : "no",
            encrypt: keyInfo.can_encrypt ? "yes" : "no",
          })}
        </Field>
        <Field labelId="security-key-source">{t(`security-source-${keyInfo.source}`)}</Field>
        {keyInfo.secret_storage !== null && (
          <Field labelId="security-key-secret">
            {t(`security-secret-${keyInfo.secret_storage}`)}
          </Field>
        )}
        <Field labelId="security-key-trust">
          <span data-part="security-trust" data-trust={keyInfo.trust}>
            {t(`security-trust-${keyInfo.trust}`)}
          </span>
        </Field>
        {keyInfo.issuer !== null && <Field labelId="security-key-issuer">{keyInfo.issuer}</Field>}
        {keyInfo.serial !== null && <Field labelId="security-key-serial">{keyInfo.serial}</Field>}
      </dl>
      {keyInfo.chain && <ChainView chain={keyInfo.chain} />}
      <TrustEditor key={`${keyInfo.id}:${keyInfo.trust}`} keyInfo={keyInfo} />
      <ExportPanel key={keyInfo.id} keyInfo={keyInfo} />
      {keyInfo.secret_storage === "passphrase" &&
        (unlocked ? (
          <p data-part="security-key.saved" role="status">
            {t("security-key-unlocked")}
          </p>
        ) : (
          <UnlockForm
            keyId={keyInfo.id}
            onUnlocked={() => {
              setUnlocked(true);
            }}
          />
        ))}
      <DeletePanel key={`del-${keyInfo.id}`} keyInfo={keyInfo} onDeleted={onDeleted} />
    </section>
  );
}
