/**
 * Reader badges (M2a): the end-to-end security badge with a details
 * popover (signer, key, trust, chain, encryption, header protection) and
 * the sender-authentication badge (DKIM / SPF / DMARC / ARC).
 *
 * `SecurityBadge` and `AuthBadge` take the message's findings as props, so
 * the reader can mount them anywhere; the layout widget `security-message`
 * shows both for the selected message (the layout decides where).
 */
import { useQueryClient } from "@tanstack/react-query";
import { useState, type FormEvent, type ReactNode } from "react";
import { errorMessageIdOf } from "../../api/client";
import type {
  AuthMethodResult,
  AuthResults,
  CertificateChain,
  KeyId,
  MessageBody,
  SecurityStatus,
  SignatureInfo,
} from "../../api/generated";
import { useLabel, useLocale, useT } from "../../i18n/I18nProvider";
import type { WidgetProps } from "../../layout/widgetRegistry";
import { useClient, useMessage } from "../../state/queries";
import { useUi } from "../../state/uiStore";
import { SelectField, TextField } from "../../widgets/forms";
import { authSummary, authTone, formatDateTime, formatFingerprint, securityVerdict } from "./model";
import { Popover } from "./Popover";
import { securityKeys } from "./queries";

/** Prefix of the reader's message queries (`queryKeys.message` in `state/queries.ts`). */
export const MESSAGE_QUERIES = ["message"] as const;

/** Minutes an unlocked key stays unlocked (`UnlockKey.remember_minutes`; "" = this session). */
export const REMEMBER_CHOICES = ["0", "15", "60", ""] as const;

function Row({ labelId, children }: { labelId: string; children: ReactNode }) {
  const t = useT();
  return (
    <div data-part="security-details.row">
      <dt data-part="security-details.label">{t(labelId)}</dt>
      <dd data-part="security-details.value">{children}</dd>
    </div>
  );
}

/** The certificate chain of an S/MIME signature, leaf first. */
export function ChainView({ chain }: { chain: CertificateChain }) {
  const t = useT();
  const locale = useLocale();
  return (
    <section data-part="security-chain" data-state={chain.state}>
      <p data-part="security-chain.state" data-state={chain.state}>
        {t(`security-chain-${chain.state}`)}
      </p>
      <p data-part="security-chain.revocation" data-state={chain.revocation}>
        {t(`security-revocation-${chain.revocation}`)}
      </p>
      <ol data-part="security-chain.list" aria-label={t("security-chain-label")}>
        {chain.certificates.map((c) => (
          <li key={c.fingerprint} data-part="security-chain.certificate">
            <span data-part="security-chain.subject">{c.subject}</span>
            <span data-part="security-chain.issuer">
              {t("security-chain-issued-by", { issuer: c.issuer })}
            </span>
            <span data-part="security-chain.validity">
              {t("security-chain-validity", {
                from: formatDateTime(c.not_before, locale, false),
                to: formatDateTime(c.not_after, locale, false),
              })}
            </span>
          </li>
        ))}
      </ol>
    </section>
  );
}

function MarkVerified({ keyId }: { keyId: KeyId }) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState(false);
  const mark = async () => {
    setError(null);
    try {
      await client.request(
        { type: "SetKeyTrust", key_id: keyId, decision: "verified" },
        "KeySaved",
      );
      setDone(true);
      await queryClient.invalidateQueries({ queryKey: securityKeys.all });
    } catch (e) {
      setError(errorMessageIdOf(e));
    }
  };
  if (done) {
    return (
      <p data-part="security-details.note" role="status">
        {t("security-details-marked-verified")}
      </p>
    );
  }
  return (
    <>
      <button
        type="button"
        data-part="form.button"
        data-action="mark-verified"
        onClick={() => void mark()}
      >
        {t("security-details-mark-verified")}
      </button>
      {error && (
        <p data-part="security-details.error" role="alert">
          {t(error)}
        </p>
      )}
    </>
  );
}

function SignatureView({ sig, messageFrom }: { sig: SignatureInfo; messageFrom: string | null }) {
  const t = useT();
  const locale = useLocale();
  return (
    <section data-part="security-details.signature" data-state={sig.state}>
      <p data-part="security-details.signature.state" data-state={sig.state}>
        {t(`security-signature-${sig.state}`)}
      </p>
      {!sig.signer_matches_from && sig.signer !== null && (
        <p data-part="security-details.warning" role="note">
          {t("security-details-signer-mismatch", {
            signer: sig.signer,
            from: messageFrom ?? "",
          })}
        </p>
      )}
      <dl data-part="security-details.list">
        {sig.signer !== null && <Row labelId="security-details-signer">{sig.signer}</Row>}
        {sig.fingerprint !== null && (
          <Row labelId="security-details-fingerprint">
            <code data-part="security-fingerprint">{formatFingerprint(sig.fingerprint)}</code>
          </Row>
        )}
        <Row labelId="security-details-trust">
          <span data-part="security-trust" data-trust={sig.trust}>
            {t(`security-trust-${sig.trust}`)}
          </span>
        </Row>
        {sig.signed_at !== null && (
          <Row labelId="security-details-signed-at">{formatDateTime(sig.signed_at, locale)}</Row>
        )}
      </dl>
      {sig.chain && <ChainView chain={sig.chain} />}
      {sig.state === "valid" &&
        sig.key_id !== null &&
        sig.chain === null &&
        (sig.trust === "unknown" ||
          sig.trust === "autocrypt" ||
          sig.trust === "autocrypt-gossip") && <MarkVerified keyId={sig.key_id} />}
      {sig.state === "unknown-key" && (
        <p data-part="security-details.note">{t("security-details-unknown-key-hint")}</p>
      )}
    </section>
  );
}

/** Passphrase prompt for a locked key (`UnlockKey`), then the message is read again. */
export function UnlockForm({ keyId, onUnlocked }: { keyId: KeyId; onUnlocked?: () => void }) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const [passphrase, setPassphrase] = useState("");
  const [remember, setRemember] = useState<string>("15");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (passphrase === "" || busy) return;
    setBusy(true);
    setError(null);
    try {
      await client.request(
        {
          type: "UnlockKey",
          key_id: keyId,
          passphrase,
          remember_minutes: remember === "" ? null : Number(remember),
        },
        "Done",
      );
      setPassphrase("");
      await Promise.all([
        // Every open message: the reader shows the decrypted body now.
        queryClient.invalidateQueries({ queryKey: MESSAGE_QUERIES }),
        queryClient.invalidateQueries({ queryKey: securityKeys.all }),
      ]);
      onUnlocked?.();
    } catch (e) {
      const id = errorMessageIdOf(e);
      setError(id === "error-auth-failed" ? "security-unlock-wrong" : id);
    } finally {
      setBusy(false);
    }
  };
  return (
    <form
      data-part="security-unlock"
      aria-label={t("security-unlock-title")}
      noValidate
      onSubmit={(e) => void submit(e)}
    >
      <TextField
        labelId="security-unlock-passphrase"
        name="unlock-passphrase"
        type="password"
        autoComplete="current-password"
        value={passphrase}
        onChange={setPassphrase}
      />
      <SelectField
        labelId="security-unlock-remember"
        name="unlock-remember"
        value={remember}
        onChange={setRemember}
        choices={REMEMBER_CHOICES.map((m) => ({
          value: m,
          label: `security-unlock-remember-${m === "" ? "session" : m}`,
        }))}
      />
      <button
        type="submit"
        data-part="form.button"
        data-variant="primary"
        data-action="unlock"
        disabled={busy || passphrase === ""}
      >
        {t("security-unlock-submit")}
      </button>
      {error && (
        <p data-part="security-unlock.error" role="alert">
          {t(error)}
        </p>
      )}
    </form>
  );
}

/** The details of a message's end-to-end security. */
export function SecurityDetails({
  status,
  messageFrom,
}: {
  status: SecurityStatus;
  messageFrom: string | null;
}) {
  const t = useT();
  const enc = status.encryption;
  return (
    <div data-part="security-details" data-technology={status.technology}>
      <p data-part="security-details.technology">{t(`security-technology-${status.technology}`)}</p>
      {status.partial && (
        <p data-part="security-details.warning" role="note">
          {t("security-details-partial")}
        </p>
      )}
      {status.inline_pgp && (
        <p data-part="security-details.note">{t("security-details-inline-pgp")}</p>
      )}
      {enc && (
        <section data-part="security-details.encryption" data-state={enc.state}>
          <p data-part="security-details.encryption.state" data-state={enc.state}>
            {t(`security-encryption-${enc.state}`)}
          </p>
          <dl data-part="security-details.list">
            {enc.algorithm !== null && (
              <Row labelId="security-details-algorithm">{enc.algorithm}</Row>
            )}
            <Row labelId="security-details-recipients">
              {t("security-details-recipient-count", { count: enc.recipients.length })}
            </Row>
          </dl>
          {enc.state === "locked" && enc.key_id !== null && <UnlockForm keyId={enc.key_id} />}
        </section>
      )}
      {status.signatures.length === 0 && (
        <p data-part="security-details.note">{t("security-details-unsigned")}</p>
      )}
      {status.signatures.map((sig, i) => (
        <SignatureView key={i} sig={sig} messageFrom={messageFrom} />
      ))}
      <p data-part="security-details.headers" data-state={status.header_protection}>
        {t(`security-headers-${status.header_protection}`)}
      </p>
    </div>
  );
}

/** The end-to-end security badge with its details popover. */
export function SecurityBadge({
  status,
  messageFrom = null,
}: {
  status: SecurityStatus;
  messageFrom?: string | null;
}) {
  const t = useT();
  const v = securityVerdict(status);
  const parts: string[] = [];
  if (v.encryption !== "none") parts.push(t(`security-badge-enc-${v.encryption}`));
  if (v.signature !== "none") parts.push(t(`security-badge-sig-${v.signature}`));
  if (v.partial) parts.push(t("security-badge-partial"));
  const text = parts.join(t("security-badge-separator"));
  return (
    <Popover
      part="security-badge"
      label={t("security-badge-label", { summary: text })}
      data={{
        tone: v.tone,
        signature: v.signature,
        encryption: v.encryption,
        technology: status.technology,
      }}
      badge={
        <>
          <span data-part="security-badge.icon" data-tone={v.tone} aria-hidden="true" />
          <span data-part="security-badge.text">{text}</span>
        </>
      }
    >
      <SecurityDetails status={status} messageFrom={messageFrom} />
    </Popover>
  );
}

const METHODS = ["dmarc", "dkim", "spf", "arc"] as const;

function MethodRow({
  method,
  result,
}: {
  method: (typeof METHODS)[number];
  result: AuthMethodResult;
}) {
  const t = useT();
  return (
    <li data-part="auth-details.method" data-method={method} data-verdict={result.verdict}>
      <span data-part="auth-details.method.name">{t(`security-auth-method-${method}`)}</span>
      <span data-part="auth-details.method.verdict" data-verdict={result.verdict}>
        {t(`security-auth-verdict-${result.verdict}`)}
      </span>
      {result.domain !== null && (
        <span data-part="auth-details.method.domain">
          {result.selector !== null
            ? t("security-auth-domain-selector", {
                domain: result.domain,
                selector: result.selector,
              })
            : result.domain}
        </span>
      )}
      {result.aligned !== null && (
        <span data-part="auth-details.method.aligned" data-aligned={String(result.aligned)}>
          {t(result.aligned ? "security-auth-aligned" : "security-auth-not-aligned")}
        </span>
      )}
      {result.reason !== null && (
        <span data-part="auth-details.method.reason">{result.reason}</span>
      )}
    </li>
  );
}

/** The sender-authentication badge (verdict of `AuthResults::summary`) with details. */
export function AuthBadge({ results }: { results: AuthResults }) {
  const t = useT();
  const verdict = authSummary(results);
  const tone = authTone(verdict);
  const rows: { method: (typeof METHODS)[number]; result: AuthMethodResult }[] = [];
  for (const method of METHODS) {
    if (method === "dkim") rows.push(...results.dkim.map((result) => ({ method, result })));
    else {
      const result = results[method];
      if (result) rows.push({ method, result });
    }
  }
  const text = t(`security-auth-badge-${verdict}`);
  return (
    <Popover
      part="auth-badge"
      label={t("security-auth-label", { summary: text })}
      data={{ tone, verdict }}
      badge={
        <>
          <span data-part="auth-badge.icon" data-tone={tone} aria-hidden="true" />
          <span data-part="auth-badge.text">{text}</span>
        </>
      }
    >
      <div data-part="auth-details">
        <p data-part="auth-details.summary">{t(`security-auth-summary-${tone}`)}</p>
        {rows.length === 0 ? (
          <p data-part="auth-details.empty">{t("security-auth-nothing")}</p>
        ) : (
          <ul data-part="auth-details.list">
            {rows.map((r, i) => (
              <MethodRow key={i} method={r.method} result={r.result} />
            ))}
          </ul>
        )}
        <p data-part="auth-details.server">
          {t("security-auth-checked-by", { server: results.authserv_id })}
        </p>
      </div>
    </Popover>
  );
}

/** Both badges for one message; renders nothing when there is nothing to show. */
export function MessageBadges({ message }: { message: MessageBody }) {
  const t = useT();
  if (!message.security && !message.auth_results) return null;
  return (
    <div data-part="security-message" role="group" aria-label={t("security-message-label")}>
      {message.security && (
        <SecurityBadge
          status={message.security}
          messageFrom={message.headers.from[0]?.email ?? null}
        />
      )}
      {message.auth_results && <AuthBadge results={message.auth_results} />}
    </div>
  );
}

/** Layout widget `security-message`: the badges of the selected message. */
export function SecurityMessageWidget({ node }: WidgetProps) {
  const label = useLabel();
  const selectedId = useUi((s) => s.selectedMessageId);
  const message = useMessage(selectedId).data;
  if (!message || message.id !== selectedId) return null;
  return (
    <div
      data-part="security-message.slot"
      data-node-id={node.id ?? undefined}
      aria-label={node.label ? label(node.label) : undefined}
    >
      <MessageBadges message={message} />
    </div>
  );
}
