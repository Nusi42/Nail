/**
 * Smartcards (M4c WP124; contract ADR 0094; desktop only):
 *
 * - widget `security-smartcards` — the PKCS#11 modules (full paths, typed
 *   or chosen with the system dialog; enabled or not), how long a card
 *   stays unlocked after its PIN (0–480 minutes), the inserted cards with
 *   their keys and certificates (`ListSmartcards`, refreshed on
 *   `SmartcardsChanged`), problems per module, and "Use for S/MIME" /
 *   "Use for OpenPGP" (`UseSmartcardKey`: a keyring entry whose secret
 *   stays on the card);
 * - the PIN dialog (`PinPrompt.tsx`).
 */
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useId, useState } from "react";
import { errorMessageOf } from "../../api/client";
import type {
  CryptoTechnology,
  Pkcs11Module,
  SmartcardObject,
  SmartcardSettings,
  SmartcardToken,
} from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { DATE_ONLY, useDateFormat } from "../../i18n/format";
import { useClient } from "../../state/queries";
import { PathField } from "../portability/PathField";
import { securityKeys } from "./queries";
import { MAX_PIN_CACHE_MINUTES, smartcardSettingsIssue } from "./smartcardModel";
import { hasText } from "../../text/empty";
import { useBusyAction } from "../../state/busyAction";

export const SMARTCARDS_WIDGET = "security-smartcards";

/** Query keys of the smartcard views. */
export const smartcardKeys = {
  settings: ["security", "smartcards", "settings"] as const,
  tokens: ["security", "smartcards", "tokens"] as const,
};

const MODULE_FILTERS = [{ label_id: "security-smartcard-module-filter", extensions: ["dll"] }];

export function useSmartcardSettings() {
  const client = useClient();
  return useQuery({
    queryKey: smartcardKeys.settings,
    retry: false,
    queryFn: async () =>
      (await client.request({ type: "GetSmartcardSettings" }, "SmartcardSettings")).settings,
  });
}

export function useSmartcards(enabled: boolean) {
  const client = useClient();
  return useQuery({
    queryKey: smartcardKeys.tokens,
    enabled,
    retry: false,
    queryFn: async () => {
      const answer = await client.request({ type: "ListSmartcards" }, "Smartcards");
      return { tokens: answer.tokens, problems: answer.problems };
    },
  });
}

function ModuleRow({
  module,
  onChange,
  onRemove,
}: {
  module: Pkcs11Module;
  onChange: (module: Pkcs11Module) => void;
  onRemove: () => void;
}) {
  const t = useT();
  return (
    <li data-part="smartcards.module" data-state={module.enabled ? "enabled" : "disabled"}>
      <label data-part="form.check">
        <input
          data-part="form.checkbox"
          type="checkbox"
          checked={module.enabled}
          onChange={(e) => {
            onChange({ ...module, enabled: e.target.checked });
          }}
        />
        <span data-part="form.check.label">{module.name ?? module.path}</span>
      </label>
      {module.name && <span data-part="smartcards.module.path">{module.path}</span>}
      <button type="button" data-part="form.button" data-action="remove-module" onClick={onRemove}>
        {t("common-remove")}
      </button>
    </li>
  );
}

function SettingsForm({ settings }: { settings: SmartcardSettings }) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const id = useId();
  const [draft, setDraft] = useState(settings);
  const [newPath, setNewPath] = useState("");
  const { busy, problem, run } = useBusyAction();
  const [saved, setSaved] = useState(false);
  const issue = smartcardSettingsIssue(draft);
  const changed = JSON.stringify(draft) !== JSON.stringify(settings);

  const save = async () => {
    await run(async () => {
      const answer = await client.request(
        { type: "SetSmartcardSettings", settings: draft },
        "SmartcardSettings",
      );
      queryClient.setQueryData(smartcardKeys.settings, answer.settings);
      void queryClient.invalidateQueries({ queryKey: smartcardKeys.tokens });
      setSaved(true);
    });
  };

  const add = () => {
    const path = newPath.trim();
    if (path === "") return;
    setDraft({ ...draft, modules: [...draft.modules, { path, name: null, enabled: true }] });
    setNewPath("");
    setSaved(false);
  };

  return (
    <section data-part="form.section" aria-label={t("security-smartcard-modules")}>
      <h4 data-part="form.section.title">{t("security-smartcard-modules")}</h4>
      <p data-part="form.hint">{t("security-smartcard-modules-hint")}</p>
      {draft.modules.length === 0 ? (
        <p data-part="form.hint" data-field="no-modules">
          {t("security-smartcard-no-modules")}
        </p>
      ) : (
        <ul data-part="smartcards.modules">
          {draft.modules.map((m, i) => (
            <ModuleRow
              key={`${m.path}-${String(i)}`}
              module={m}
              onChange={(next) => {
                setDraft({ ...draft, modules: draft.modules.map((x, j) => (j === i ? next : x)) });
                setSaved(false);
              }}
              onRemove={() => {
                setDraft({ ...draft, modules: draft.modules.filter((_, j) => j !== i) });
                setSaved(false);
              }}
            />
          ))}
        </ul>
      )}
      <PathField
        labelId="security-smartcard-module-path"
        field="module-path"
        value={newPath}
        onChange={setNewPath}
        kind="open-file"
        titleId="security-smartcard-module-pick-title"
        filters={MODULE_FILTERS}
      />
      <button
        type="button"
        data-part="form.button"
        data-action="add-module"
        disabled={newPath.trim() === ""}
        onClick={add}
      >
        {t("security-smartcard-add-module")}
      </button>
      <div data-part="form.field" data-field="pin-cache">
        <label data-part="form.label" htmlFor={`${id}-cache`}>
          {t("security-smartcard-pin-cache")}
        </label>
        <input
          id={`${id}-cache`}
          data-part="form.input"
          data-width="short"
          type="number"
          min={0}
          max={MAX_PIN_CACHE_MINUTES}
          value={draft.pin_cache_minutes}
          aria-describedby={`${id}-cache-hint`}
          onChange={(e) => {
            setDraft({
              ...draft,
              pin_cache_minutes: Math.max(0, Math.round(Number(e.target.value) || 0)),
            });
            setSaved(false);
          }}
        />
        <p data-part="form.hint" id={`${id}-cache-hint`}>
          {t("security-smartcard-pin-cache-hint")}
        </p>
      </div>
      {issue && (
        <p data-part="form.hint" data-tone="danger">
          {t(issue)}
        </p>
      )}
      {problem && (
        <p data-part="form.error" role="alert">
          {t(problem.id, problem.args)}
        </p>
      )}
      {saved && !changed && (
        <p data-part="form.hint" role="status" data-tone="success">
          {t("common-saved")}
        </p>
      )}
      <button
        type="button"
        data-part="form.button"
        data-variant="primary"
        data-action="save-smartcards"
        disabled={busy || !changed || issue !== null}
        onClick={() => void save()}
      >
        {t("common-save")}
      </button>
    </section>
  );
}

function ObjectRow({ token, object }: { token: SmartcardToken; object: SmartcardObject }) {
  const t = useT();
  const format = useDateFormat();
  const client = useClient();
  const queryClient = useQueryClient();
  const { busy, problem, run } = useBusyAction();
  const cert = object.certificate;

  const use = async (technology: CryptoTechnology) => {
    await run(async () => {
      await client.request(
        {
          type: "UseSmartcardKey",
          key: { module: token.module, token_serial: token.serial, object_id: object.id },
          technology,
        },
        "Key",
      );
      void queryClient.invalidateQueries({ queryKey: smartcardKeys.tokens });
      void queryClient.invalidateQueries({ queryKey: securityKeys.all });
    });
  };

  return (
    <li data-part="smartcards.object" data-kind={object.kind}>
      <span data-part="smartcards.object.kind">{t(`smartcard-object-${object.kind}`)}</span>
      <span data-part="smartcards.object.label">{object.label}</span>
      {object.algorithm && <span data-part="smartcards.object.algorithm">{object.algorithm}</span>}
      {object.usages.length > 0 && (
        <span data-part="smartcards.object.usages">
          {object.usages.map((u) => t(`smartcard-usage-${u}`)).join(", ")}
        </span>
      )}
      {cert && (
        <span data-part="smartcards.object.certificate">
          {t("security-smartcard-certificate", {
            subject: cert.subject,
            until: format.dateTime(cert.not_after, DATE_ONLY),
          })}
        </span>
      )}
      {object.openpgp_fingerprint && (
        <span data-part="smartcards.object.fingerprint">
          {t("security-smartcard-openpgp", { fingerprint: object.openpgp_fingerprint })}
        </span>
      )}
      {object.kind === "private-key" &&
        (object.key_id ? (
          <span data-part="smartcards.object.used" data-state="in-keyring">
            {t("security-smartcard-in-keyring")}
          </span>
        ) : (
          <span data-part="smartcards.object.actions">
            {cert && (
              <button
                type="button"
                data-part="form.button"
                data-action="use-smime"
                disabled={busy}
                onClick={() => void use("smime")}
              >
                {t("security-smartcard-use-smime")}
              </button>
            )}
            {(hasText(object.openpgp_fingerprint) || !cert) && (
              <button
                type="button"
                data-part="form.button"
                data-action="use-openpgp"
                disabled={busy}
                onClick={() => void use("openpgp")}
              >
                {t("security-smartcard-use-openpgp")}
              </button>
            )}
          </span>
        ))}
      {problem && (
        <p data-part="form.error" role="alert">
          {t(problem.id, problem.args)}
        </p>
      )}
    </li>
  );
}

function TokenCard({ token }: { token: SmartcardToken }) {
  const t = useT();
  return (
    <li
      data-part="smartcards.token"
      data-state={token.pin_locked ? "locked" : token.pin_count_low ? "low" : undefined}
    >
      <h5 data-part="smartcards.token.label">{token.label}</h5>
      <p data-part="smartcards.token.facts">
        {t("security-smartcard-token", {
          manufacturer: token.manufacturer,
          model: token.model,
          serial: token.serial,
        })}
      </p>
      {token.pin_locked && (
        <p data-part="form.hint" data-tone="danger">
          {t("smartcard-error-pin-locked")}
        </p>
      )}
      {!token.pin_locked && token.pin_count_low && (
        <p data-part="form.hint" data-tone="warning">
          {t("security-smartcard-pin-low")}
        </p>
      )}
      <ul data-part="smartcards.objects">
        {token.objects.map((o, i) => (
          <ObjectRow key={`${o.kind}-${o.id}-${String(i)}`} token={token} object={o} />
        ))}
      </ul>
    </li>
  );
}

/** Widget `security-smartcards`. */
export function SmartcardsWidget() {
  const t = useT();
  const settings = useSmartcardSettings();
  const enabled = (settings.data?.modules ?? []).some((m) => m.enabled);
  const cards = useSmartcards(settings.isSuccess && enabled);
  if (settings.isError) {
    return (
      <section data-part="smartcards" data-state="unavailable">
        <h3 data-part="form.section.title">{t("security-smartcard-title")}</h3>
        <p data-part="form.hint">{t("security-smartcard-desktop-only")}</p>
      </section>
    );
  }
  if (!settings.data) return null;
  return (
    <section data-part="smartcards">
      <h3 data-part="form.section.title">{t("security-smartcard-title")}</h3>
      <p data-part="form.hint">{t("security-smartcard-intro")}</p>
      {/* New settings from the core start a fresh form. */}
      <SettingsForm key={JSON.stringify(settings.data)} settings={settings.data} />
      <section data-part="form.section" aria-label={t("security-smartcard-cards")}>
        <h4 data-part="form.section.title">{t("security-smartcard-cards")}</h4>
        {!enabled ? (
          <p data-part="form.hint">{t("security-smartcard-enable-first")}</p>
        ) : cards.isError ? (
          <p data-part="form.error" role="alert">
            {t(errorMessageOf(cards.error).id)}
          </p>
        ) : (
          <>
            {(cards.data?.problems ?? []).map((p) => (
              <p
                key={p.module}
                data-part="form.hint"
                data-tone="danger"
                data-field="module-problem"
              >
                {t("security-smartcard-module-problem", {
                  module: p.module,
                  problem: t(`smartcard-error-${p.error}`),
                })}
              </p>
            ))}
            {cards.data?.tokens.length === 0 ? (
              <p data-part="form.hint" data-field="no-cards">
                {t("security-smartcard-no-cards")}
              </p>
            ) : (
              <ul data-part="smartcards.tokens">
                {(cards.data?.tokens ?? []).map((token) => (
                  <TokenCard key={`${token.module}-${token.serial}`} token={token} />
                ))}
              </ul>
            )}
          </>
        )}
      </section>
    </section>
  );
}
