/**
 * Composer sign / encrypt toggles with live recipient key status
 * (`CheckRecipientKeys`, debounced), the Autocrypt recommendation, online
 * lookup of missing keys, and the send guard: a chosen protection that
 * cannot work is explained, never silently dropped (ADR 0029).
 *
 * `ComposerSecurity` is controlled (`choice` / `onChoice`), so any composer
 * can mount it. The layout widget `security-composer` binds it to the
 * composer of its own window (M5c, `composerOf`) and `composerCryptoStore`
 * (the focused composer's choice; the composer swaps it on focus changes).
 */
import { useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useState } from "react";
import { errorMessageIdOf } from "../../api/client";
import type {
  CryptoTechnology,
  IdentityId,
  RecipientKeyStatus,
  RecipientKeysReport,
  SendCrypto,
} from "../../api/generated";
import { useLabel, useT } from "../../i18n/I18nProvider";
import type { WidgetProps } from "../../layout/widgetRegistry";
import { parseAddressList } from "../../state/addresses";
import { useClient, useIdentities } from "../../state/queries";
import { useUi } from "../../state/uiStore";
import { CheckField, SelectField } from "../../widgets/forms";
import { composerCryptoStore, useComposerCrypto } from "./composerStore";
import {
  TECHNOLOGIES,
  defaultIdentityCrypto,
  plainChoice,
  recipientKeyOf,
  recipientState,
  resolveTechnology,
  sendBlock,
} from "./model";
import { securityKeys, useRecipientKeys, useSecuritySettings } from "./queries";
import { composerOf, useComposerWindowOf } from "../composer/windowOf";

function RecipientRow({
  status,
  technology,
  onImported,
}: {
  status: RecipientKeyStatus;
  technology: CryptoTechnology;
  onImported: () => void;
}) {
  const t = useT();
  const client = useClient();
  const settings = useSecuritySettings().data;
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const key = recipientKeyOf(status, technology);
  const state = recipientState(status, technology);

  const importFound = async () => {
    if (!settings) return;
    setBusy(true);
    setError(null);
    try {
      const found = await client.request(
        { type: "LookupKeys", email: status.email, sources: settings.lookup_sources },
        "KeyLookup",
      );
      const first = found.candidates[0];
      if (!first) {
        setError("security-compose-lookup-gone");
        return;
      }
      await client.request(
        { type: "ImportKeys", data: first.armored, encoding: "text" },
        "KeysImported",
      );
      onImported();
    } catch (e) {
      setError(errorMessageIdOf(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <li
      data-part="security-compose.recipient"
      data-state={state}
      data-trust={key?.trust}
      data-email={status.email}
    >
      <span data-part="security-compose.recipient.email">{status.email}</span>
      <span data-part="security-compose.recipient.state" data-state={state}>
        {state === "usable" && key
          ? t("security-compose-key-usable", { trust: t(`security-trust-${key.trust}`) })
          : t(`security-compose-key-${state}`)}
      </span>
      {status.autocrypt?.prefer_encrypt === "mutual" && (
        <span data-part="security-compose.recipient.autocrypt">
          {t("security-compose-autocrypt-mutual")}
        </span>
      )}
      {technology === "openpgp" && state !== "usable" && status.lookup !== "not-attempted" && (
        <span data-part="security-compose.recipient.lookup" data-state={status.lookup}>
          {t(`security-compose-lookup-${status.lookup}`)}
        </span>
      )}
      {technology === "openpgp" && state !== "usable" && status.lookup === "found" && (
        <button
          type="button"
          data-part="form.button"
          data-action="import-found"
          disabled={busy}
          onClick={() => void importFound()}
        >
          {t("security-compose-import-found")}
        </button>
      )}
      {error && (
        <span data-part="security-compose.error" role="alert">
          {t(error)}
        </span>
      )}
    </li>
  );
}

export interface ComposerSecurityProps {
  identityId: IdentityId | null;
  /** Every recipient address (To, Cc, Bcc). */
  recipients: readonly string[];
  /** The explicit choice, `null` = follow the core's suggestion. */
  choice: SendCrypto | null;
  onChoice: (choice: SendCrypto | null) => void;
  /** Receives every new recipient report (for a send guard outside). */
  onReport?: (report: RecipientKeysReport | null) => void;
}

/** Sign / encrypt toggles and recipient key status for one message. */
export function ComposerSecurity({
  identityId,
  recipients,
  choice,
  onChoice,
  onReport,
}: ComposerSecurityProps) {
  const t = useT();
  const queryClient = useQueryClient();
  const settings = useSecuritySettings().data;
  const identity = (useIdentities(null).data ?? []).find((i) => i.id === identityId) ?? null;
  const preference = (identity?.crypto ?? defaultIdentityCrypto()).technology;
  const [lookupAsked, setLookupAsked] = useState(false);
  const lookup =
    settings !== undefined &&
    settings.key_lookup !== "off" &&
    (settings.key_lookup === "automatic" || lookupAsked);
  const check = useRecipientKeys(identityId, recipients, lookup);
  const report = check.data ?? null;

  useEffect(() => {
    onReport?.(report);
  }, [report, onReport]);

  const effective = choice ?? report?.suggested ?? plainChoice();
  const technology = resolveTechnology(effective, preference, report);
  const block = check.pending || check.isFetching ? null : sendBlock(effective, preference, report);
  const set = (patch: Partial<SendCrypto>) => {
    onChoice({ ...effective, ...patch });
  };
  const missing = (report?.recipients ?? []).filter(
    (r) => recipientState(r, "openpgp") !== "usable",
  );
  const refresh = () => queryClient.invalidateQueries({ queryKey: securityKeys.all });

  return (
    <section
      data-part="security-compose"
      data-sign={effective.sign ? "" : undefined}
      data-encrypt={effective.encrypt ? "" : undefined}
      data-technology={technology}
      aria-label={t("security-compose-label")}
    >
      <div data-part="security-compose.toggles">
        <CheckField
          labelId="security-compose-sign"
          name="compose-sign"
          checked={effective.sign}
          onChange={(sign) => {
            set({ sign });
          }}
        />
        <CheckField
          labelId="security-compose-encrypt"
          name="compose-encrypt"
          checked={effective.encrypt}
          onChange={(encrypt) => {
            set({ encrypt });
          }}
        />
        <SelectField
          labelId="security-compose-technology"
          name="compose-technology"
          value={effective.technology ?? ""}
          onChange={(v) => {
            set({ technology: v === "" ? null : (v as CryptoTechnology) });
          }}
          choices={[
            { value: "", label: "security-compose-technology-auto" },
            ...TECHNOLOGIES.map((tech) => ({ value: tech, label: `security-technology-${tech}` })),
          ]}
        />
        {choice !== null && (
          <button
            type="button"
            data-part="form.button"
            data-action="crypto-defaults"
            onClick={() => {
              onChoice(null);
            }}
          >
            {t("security-compose-use-defaults")}
          </button>
        )}
      </div>
      {report && report.autocrypt !== "disable" && (
        <p data-part="security-compose.autocrypt" data-state={report.autocrypt}>
          {t(`security-compose-autocrypt-${report.autocrypt}`)}
        </p>
      )}
      {check.isError && (
        <p data-part="security-compose.error" role="alert">
          {t(errorMessageIdOf(check.error))}
        </p>
      )}
      {report && report.recipients.length > 0 && (
        <ul data-part="security-compose.recipients" aria-label={t("security-compose-recipients")}>
          {report.recipients.map((r) => (
            <RecipientRow
              key={r.email}
              status={r}
              technology={technology}
              onImported={() => void refresh()}
            />
          ))}
        </ul>
      )}
      {settings?.key_lookup === "on-demand" &&
        !lookupAsked &&
        technology === "openpgp" &&
        missing.length > 0 && (
          <button
            type="button"
            data-part="form.button"
            data-action="lookup-missing"
            onClick={() => {
              setLookupAsked(true);
            }}
          >
            {t("security-compose-lookup", { count: missing.length })}
          </button>
        )}
      {block && block.id !== "security-compose-block-checking" && (
        <p data-part="security-compose.block" role="alert" data-reason={block.id}>
          {t(block.id, {
            count: block.recipients.length,
            recipients: block.recipients.join(", "),
          })}
        </p>
      )}
    </section>
  );
}

/** Recipient addresses from the composer's typed fields. */
export function composerRecipients(fields: { to: string; cc: string; bcc: string }): string[] {
  return [fields.to, fields.cc, fields.bcc].flatMap((f) =>
    parseAddressList(f).addresses.map((a) => a.email),
  );
}

/** Layout widget `security-composer`: the toggles for the open composer. */
export function ComposerSecurityWidget({ node }: WidgetProps) {
  const label = useLabel();
  // The composer of this widget's own window (M5c: several composers).
  const window = useComposerWindowOf();
  const composer = useUi((s) => composerOf(s, window));
  const choice = useComposerCrypto((s) => s.choice);
  const identityId = composer?.identityId ?? null;
  const identity = (useIdentities(null).data ?? []).find((i) => i.id === identityId) ?? null;
  const preference = (identity?.crypto ?? defaultIdentityCrypto()).technology;
  const open = composer !== null;
  const onReport = useCallback(
    (r: RecipientKeysReport | null) => {
      composerCryptoStore.getState().setReport(r, preference);
    },
    [preference],
  );
  const onChoice = useCallback((c: SendCrypto | null) => {
    composerCryptoStore.getState().setChoice(c);
  }, []);

  // A closed composer forgets its choice; the next message starts from the defaults.
  useEffect(() => {
    if (!open) composerCryptoStore.getState().reset();
  }, [open]);

  if (!composer) return null;
  const recipients = composerRecipients(composer);
  return (
    <div
      data-part="security-compose.slot"
      data-node-id={node.id ?? undefined}
      aria-label={node.label ? label(node.label) : undefined}
    >
      <ComposerSecurity
        identityId={identityId}
        recipients={recipients}
        choice={choice}
        onChoice={onChoice}
        onReport={onReport}
      />
    </div>
  );
}
