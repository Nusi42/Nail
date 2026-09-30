/**
 * Tools of the key manager: import (paste, key files, PKCS#12), generate
 * an OpenPGP key, look up a key online, import from the Windows
 * certificate store.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";
import { errorMessageId, errorMessageIdOf, NailError } from "../../api/client";
import type {
  CryptoKey,
  KeyAlgorithm,
  KeyImportResult,
  Response,
  SecretProtection,
  WindowsCertStore,
} from "../../api/generated";
import { useLocale, useT } from "../../i18n/I18nProvider";
import { useClient, useIdentities } from "../../state/queries";
import { SelectField, TextAreaField, TextField } from "../../widgets/forms";
import { identityText } from "./IdentityCrypto";
import {
  bytesToBase64,
  formatDateTime,
  formatFingerprint,
  isPkcs12Name,
  looksArmored,
} from "./model";
import { securityKeys, useSecuritySettings, useWindowsCertificates } from "./queries";

/** Called with the keys a tool added or changed (the manager selects the first). */
export type OnKeys = (keys: CryptoKey[]) => void;

function useInvalidateKeys() {
  const queryClient = useQueryClient();
  return () => queryClient.invalidateQueries({ queryKey: securityKeys.all });
}

function ErrorLine({ id }: { id: string | null }) {
  const t = useT();
  if (!id) return null;
  return (
    <p data-part="security-tool.error" role="alert">
      {t(id)}
    </p>
  );
}

/** "2 new, 1 updated" plus skipped items. */
export function ImportResult({ result }: { result: KeyImportResult }) {
  const t = useT();
  return (
    <div data-part="security-import.result" role="status">
      <p data-part="security-import.summary">
        {t("security-import-summary", { created: result.created, updated: result.updated })}
      </p>
      {result.issues.length > 0 && (
        <ul data-part="security-import.issues" aria-label={t("security-import-issues")}>
          {result.issues.map((issue, i) => (
            <li key={i} data-part="security-import.issue" data-code={issue.code}>
              {t("security-import-issue", {
                item: issue.item,
                message: issue.localized
                  ? t(issue.localized.id, issue.localized.args)
                  : issue.message,
              })}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Protection choice (keychain / passphrase), shared by import and generate

function ProtectionFields({
  value,
  onChange,
}: {
  value: { kind: "keychain" | "passphrase"; passphrase: string; confirm: string };
  onChange: (v: { kind: "keychain" | "passphrase"; passphrase: string; confirm: string }) => void;
}) {
  const t = useT();
  return (
    <div data-part="security-protection">
      <SelectField
        labelId="security-protection"
        name="protection"
        value={value.kind}
        onChange={(kind) => onChange({ ...value, kind: kind as "keychain" | "passphrase" })}
        choices={[
          { value: "keychain", label: "security-protection-keychain" },
          { value: "passphrase", label: "security-protection-passphrase" },
        ]}
      />
      <p data-part="form.hint">{t(`security-protection-${value.kind}-hint`)}</p>
      {value.kind === "passphrase" && (
        <>
          <TextField
            labelId="security-protection-new-passphrase"
            name="new-passphrase"
            type="password"
            autoComplete="new-password"
            value={value.passphrase}
            onChange={(passphrase) => onChange({ ...value, passphrase })}
          />
          <TextField
            labelId="security-protection-confirm"
            name="confirm-passphrase"
            type="password"
            autoComplete="new-password"
            value={value.confirm}
            onChange={(confirm) => onChange({ ...value, confirm })}
          />
        </>
      )}
    </div>
  );
}

type ProtectionInput = Parameters<typeof ProtectionFields>[0]["value"];

function protectionProblem(p: ProtectionInput): string | null {
  if (p.kind === "keychain") return null;
  if (p.passphrase === "") return "security-protection-empty";
  if (p.passphrase !== p.confirm) return "security-protection-mismatch";
  return null;
}

function protectionOf(p: ProtectionInput): SecretProtection {
  return p.kind === "keychain"
    ? { kind: "keychain" }
    : { kind: "passphrase", passphrase: p.passphrase };
}

const NO_PROTECTION: ProtectionInput = { kind: "keychain", passphrase: "", confirm: "" };

// ---------------------------------------------------------------------------
// Import

interface PendingP12 {
  name: string;
  data: string;
}

export function ImportTool({ onKeys }: { onKeys: OnKeys }) {
  const t = useT();
  const client = useClient();
  const invalidate = useInvalidateKeys();
  const [text, setText] = useState("");
  const [p12, setP12] = useState<PendingP12 | null>(null);
  const [p12Pass, setP12Pass] = useState("");
  const [protection, setProtection] = useState<ProtectionInput>(NO_PROTECTION);
  const [result, setResult] = useState<KeyImportResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const run = async (request: Parameters<typeof client.request>[0]) => {
    setBusy(true);
    setError(null);
    setResult(null);
    try {
      const answer = await client.request(request, "KeysImported");
      setResult(answer.result);
      await invalidate();
      onKeys(answer.result.keys);
      return true;
    } catch (e) {
      const id = errorMessageIdOf(e);
      setError(
        id === "error-auth-failed" && request.type === "ImportPkcs12"
          ? "security-import-p12-wrong"
          : id === "error-crypto"
            ? "security-import-nothing"
            : id,
      );
      return false;
    } finally {
      setBusy(false);
    }
  };

  const importText = async (event: FormEvent) => {
    event.preventDefault();
    if (text.trim() === "" || busy) return;
    if (await run({ type: "ImportKeys", data: text, encoding: "text" })) setText("");
  };

  const pickFile = async (file: File | undefined) => {
    if (!file) return;
    setError(null);
    setResult(null);
    const bytes = new Uint8Array(await file.arrayBuffer());
    if (isPkcs12Name(file.name)) {
      setP12({ name: file.name, data: bytesToBase64(bytes) });
      return;
    }
    const asText = new TextDecoder("utf-8", { fatal: false }).decode(bytes);
    await run(
      looksArmored(asText)
        ? { type: "ImportKeys", data: asText, encoding: "text" }
        : { type: "ImportKeys", data: bytesToBase64(bytes), encoding: "base64" },
    );
  };

  const importP12 = async (event: FormEvent) => {
    event.preventDefault();
    if (!p12 || busy) return;
    const problem = protectionProblem(protection);
    if (problem) {
      setError(problem);
      return;
    }
    const ok = await run({
      type: "ImportPkcs12",
      data: p12.data,
      passphrase: p12Pass,
      protection: protectionOf(protection),
    });
    if (ok) {
      setP12(null);
      setP12Pass("");
      setProtection(NO_PROTECTION);
    }
  };

  return (
    <section data-part="security-tool" data-tool="import" aria-label={t("security-tool-import")}>
      <p data-part="security-tool.intro">{t("security-import-intro")}</p>
      <form data-part="security-import.paste" noValidate onSubmit={(e) => void importText(e)}>
        <TextAreaField
          labelId="security-import-paste"
          name="import-text"
          value={text}
          onChange={setText}
        />
        <button
          type="submit"
          data-part="form.button"
          data-variant="primary"
          data-action="import-text"
          disabled={busy || text.trim() === ""}
        >
          {t("security-import-submit")}
        </button>
      </form>
      <label data-part="form.field" data-field="import-file">
        <span data-part="form.label">{t("security-import-file")}</span>
        <input
          data-part="form.file"
          type="file"
          name="import-file"
          accept=".asc,.gpg,.pgp,.key,.pem,.crt,.cer,.der,.p12,.pfx"
          onChange={(event) => {
            void pickFile(event.target.files?.[0]);
            event.target.value = "";
          }}
        />
      </label>
      {p12 && (
        <form
          data-part="security-import.p12"
          aria-label={t("security-import-p12-title", { name: p12.name })}
          noValidate
          onSubmit={(e) => void importP12(e)}
        >
          <p data-part="security-tool.intro">
            {t("security-import-p12-title", { name: p12.name })}
          </p>
          <TextField
            labelId="security-import-p12-passphrase"
            name="p12-passphrase"
            type="password"
            autoComplete="off"
            value={p12Pass}
            onChange={setP12Pass}
          />
          <ProtectionFields value={protection} onChange={setProtection} />
          <button
            type="submit"
            data-part="form.button"
            data-variant="primary"
            data-action="import-p12"
            disabled={busy}
          >
            {t("security-import-p12-submit")}
          </button>
          <button
            type="button"
            data-part="form.button"
            data-action="cancel"
            onClick={() => setP12(null)}
          >
            {t("security-cancel")}
          </button>
        </form>
      )}
      <ErrorLine id={error} />
      {result && <ImportResult result={result} />}
    </section>
  );
}

// ---------------------------------------------------------------------------
// Generate

/** Expiry choices in days ("" = never). */
export const EXPIRY_CHOICES = ["365", "730", "1825", ""] as const;

export function GenerateTool({ onKeys }: { onKeys: OnKeys }) {
  const t = useT();
  const client = useClient();
  const invalidate = useInvalidateKeys();
  const identities = useIdentities(null).data ?? [];
  const [identity, setIdentity] = useState<string>("");
  const [algorithm, setAlgorithm] = useState<KeyAlgorithm>("curve25519");
  const [expiry, setExpiry] = useState<string>("730");
  const [protection, setProtection] = useState<ProtectionInput>(NO_PROTECTION);
  const [created, setCreated] = useState<CryptoKey | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const chosen = identity || identities[0]?.id || "";

  const generate = async (event: FormEvent) => {
    event.preventDefault();
    if (chosen === "" || busy) return;
    const problem = protectionProblem(protection);
    if (problem) {
      setError(problem);
      return;
    }
    setBusy(true);
    setError(null);
    setCreated(null);
    try {
      const answer = await client.request(
        {
          type: "GenerateKey",
          identity_id: chosen,
          algorithm,
          expires_days: expiry === "" ? null : Number(expiry),
          protection: protectionOf(protection),
        },
        "KeyGenerated",
      );
      setCreated(answer.key);
      setProtection(NO_PROTECTION);
      await invalidate();
      onKeys([answer.key]);
    } catch (e) {
      setError(errorMessageIdOf(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section
      data-part="security-tool"
      data-tool="generate"
      aria-label={t("security-tool-generate")}
    >
      <form noValidate data-part="security-generate" onSubmit={(e) => void generate(e)}>
        <p data-part="security-tool.intro">{t("security-generate-intro")}</p>
        <SelectField
          labelId="security-generate-identity"
          name="generate-identity"
          value={chosen}
          onChange={setIdentity}
          choices={identities.map((i) => ({ value: i.id, label: identityText(i), literal: true }))}
        />
        <SelectField
          labelId="security-generate-algorithm"
          name="generate-algorithm"
          value={algorithm}
          onChange={(v) => setAlgorithm(v as KeyAlgorithm)}
          choices={[
            { value: "curve25519", label: "security-algorithm-curve25519" },
            { value: "rsa4096", label: "security-algorithm-rsa4096" },
          ]}
        />
        <SelectField
          labelId="security-generate-expiry"
          name="generate-expiry"
          value={expiry}
          onChange={setExpiry}
          choices={EXPIRY_CHOICES.map((d) => ({
            value: d,
            label: `security-expiry-${d === "" ? "never" : d}`,
          }))}
        />
        <ProtectionFields value={protection} onChange={setProtection} />
        <button
          type="submit"
          data-part="form.button"
          data-variant="primary"
          data-action="generate"
          disabled={busy || chosen === ""}
        >
          {t(busy ? "security-generate-busy" : "security-generate-submit")}
        </button>
      </form>
      <ErrorLine id={error} />
      {created && (
        <p data-part="security-generate.done" role="status">
          {t("security-generate-done", { fingerprint: formatFingerprint(created.fingerprint) })}
        </p>
      )}
    </section>
  );
}

// ---------------------------------------------------------------------------
// Lookup

type Lookup = Extract<Response, { type: "KeyLookup" }>;

export function LookupTool({ onKeys }: { onKeys: OnKeys }) {
  const t = useT();
  const locale = useLocale();
  const client = useClient();
  const invalidate = useInvalidateKeys();
  const settings = useSecuritySettings().data;
  const [email, setEmail] = useState("");
  const [found, setFound] = useState<Lookup | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [importedFps, setImportedFps] = useState<string[]>([]);
  const off = settings?.key_lookup === "off";

  const search = async (event: FormEvent) => {
    event.preventDefault();
    if (email.trim() === "" || busy || !settings) return;
    setBusy(true);
    setError(null);
    setFound(null);
    try {
      setFound(
        await client.request(
          { type: "LookupKeys", email: email.trim(), sources: settings.lookup_sources },
          "KeyLookup",
        ),
      );
    } catch (e) {
      setError(
        e instanceof NailError && e.code === "unsupported"
          ? "security-lookup-off"
          : errorMessageIdOf(e),
      );
    } finally {
      setBusy(false);
    }
  };

  const take = async (armored: string, fingerprint: string) => {
    setError(null);
    try {
      const answer = await client.request(
        { type: "ImportKeys", data: armored, encoding: "text" },
        "KeysImported",
      );
      setImportedFps((l) => [...l, fingerprint]);
      await invalidate();
      onKeys(answer.result.keys);
    } catch (e) {
      setError(errorMessageIdOf(e));
    }
  };

  return (
    <section data-part="security-tool" data-tool="lookup" aria-label={t("security-tool-lookup")}>
      <p data-part="security-tool.intro">{t("security-lookup-intro")}</p>
      {off && (
        <p data-part="security-tool.note" role="note">
          {t("security-lookup-off")}
        </p>
      )}
      <form data-part="security-lookup" noValidate onSubmit={(e) => void search(e)}>
        <TextField
          labelId="security-lookup-email"
          name="lookup-email"
          type="email"
          value={email}
          onChange={setEmail}
        />
        <button
          type="submit"
          data-part="form.button"
          data-variant="primary"
          data-action="lookup"
          disabled={busy || off || email.trim() === "" || !settings}
        >
          {t(busy ? "security-lookup-busy" : "security-lookup-submit")}
        </button>
      </form>
      <ErrorLine id={error} />
      {found && (
        <div data-part="security-lookup.result" role="status">
          {found.candidates.length === 0 && (
            <p data-part="security-lookup.none">
              {t("security-lookup-none", { email: found.email })}
            </p>
          )}
          {found.candidates.length > 0 && (
            <ul data-part="security-lookup.list" aria-label={t("security-lookup-found")}>
              {found.candidates.map((c) => {
                const done = c.known !== null || importedFps.includes(c.fingerprint);
                return (
                  <li key={`${c.source}:${c.fingerprint}`} data-part="security-lookup.candidate">
                    <span data-part="security-lookup.user-ids">{c.user_ids.join(", ")}</span>
                    <code data-part="security-fingerprint">{formatFingerprint(c.fingerprint)}</code>
                    <span data-part="security-lookup.meta">
                      {t("security-lookup-meta", {
                        source: t(`security-lookup-source-${c.source}`),
                        algorithm: c.algorithm,
                        created: formatDateTime(c.created_at, locale, false),
                      })}
                    </span>
                    {done ? (
                      <span data-part="security-lookup.known">{t("security-lookup-known")}</span>
                    ) : (
                      <button
                        type="button"
                        data-part="form.button"
                        data-action="import-candidate"
                        onClick={() => void take(c.armored, c.fingerprint)}
                      >
                        {t("security-lookup-import")}
                      </button>
                    )}
                  </li>
                );
              })}
            </ul>
          )}
          {found.failures.map((f) => (
            <p key={f.source} data-part="security-lookup.failure" data-code={f.code}>
              {t("security-lookup-failed", {
                source: t(`security-lookup-source-${f.source}`),
                error: t(errorMessageId(f.code)),
              })}
            </p>
          ))}
          {found.candidates.length > 0 && (
            <p data-part="form.hint">{t("security-lookup-verify-hint")}</p>
          )}
        </div>
      )}
    </section>
  );
}

// ---------------------------------------------------------------------------
// Windows certificate store

export const WINDOWS_STORES: readonly WindowsCertStore[] = ["personal", "other-people"];

export function WindowsStoreTool({ onKeys }: { onKeys: OnKeys }) {
  const t = useT();
  const locale = useLocale();
  const client = useClient();
  const invalidate = useInvalidateKeys();
  const [store, setStore] = useState<WindowsCertStore>("personal");
  const certs = useWindowsCertificates(store, true);
  const [error, setError] = useState<string | null>(null);

  const take = async (thumbprint: string) => {
    setError(null);
    try {
      const answer = await client.request(
        { type: "ImportWindowsCertificate", store, thumbprint },
        "KeysImported",
      );
      await Promise.all([invalidate(), certs.refetch()]);
      onKeys(answer.result.keys);
    } catch (e) {
      setError(errorMessageIdOf(e));
    }
  };

  const unsupported = certs.error instanceof NailError && certs.error.code === "unsupported";
  return (
    <section data-part="security-tool" data-tool="windows" aria-label={t("security-tool-windows")}>
      <p data-part="security-tool.intro">{t("security-windows-intro")}</p>
      <SelectField
        labelId="security-windows-store"
        name="windows-store"
        value={store}
        onChange={(v) => setStore(v as WindowsCertStore)}
        choices={WINDOWS_STORES.map((s) => ({ value: s, label: `security-windows-store-${s}` }))}
      />
      {unsupported && (
        <p data-part="security-tool.note" role="note">
          {t("security-windows-unsupported")}
        </p>
      )}
      {certs.isError && !unsupported && <ErrorLine id={errorMessageIdOf(certs.error)} />}
      {certs.isSuccess && certs.data.length === 0 && (
        <p data-part="security-tool.note">{t("security-windows-empty")}</p>
      )}
      {certs.isSuccess && certs.data.length > 0 && (
        <ul data-part="security-windows.list" aria-label={t(`security-windows-store-${store}`)}>
          {certs.data.map((c) => (
            <li
              key={c.thumbprint}
              data-part="security-windows.certificate"
              data-usable={String(c.usable_for_email)}
            >
              <span data-part="security-windows.subject">{c.certificate.subject}</span>
              <span data-part="security-windows.meta">
                {t("security-chain-issued-by", { issuer: c.certificate.issuer })}
              </span>
              <span data-part="security-windows.meta">
                {t("security-chain-validity", {
                  from: formatDateTime(c.certificate.not_before, locale, false),
                  to: formatDateTime(c.certificate.not_after, locale, false),
                })}
              </span>
              {c.emails.length > 0 && (
                <span data-part="security-windows.emails">{c.emails.join(", ")}</span>
              )}
              {c.has_private_key && (
                <span data-part="security-windows.private">
                  {t("security-windows-private-key")}
                </span>
              )}
              {!c.usable_for_email ? (
                <span data-part="security-windows.unusable">
                  {t("security-windows-not-for-email")}
                </span>
              ) : c.imported !== null ? (
                <span data-part="security-windows.imported">{t("security-windows-imported")}</span>
              ) : (
                <button
                  type="button"
                  data-part="form.button"
                  data-action="import-windows"
                  onClick={() => void take(c.thumbprint)}
                >
                  {t("security-windows-import")}
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
      <ErrorLine id={error} />
    </section>
  );
}
