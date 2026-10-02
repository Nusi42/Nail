/**
 * The diagnostics bundle (dialog `diagnostics-bundle`, action
 * `diagnostics.open`, widget `diagnostics`): user-initiated, nothing is
 * sent anywhere. The user chooses what goes in (no mail content by
 * default; headers or full messages only for messages picked one by one),
 * whether addresses and subjects are replaced, then **sees exactly what
 * the zip will hold** (`PreviewDiagnostics`: every file, its size, what was
 * removed, the start of its text) before saving it
 * (`CreateDiagnosticsBundle` with the same options). Changing an option
 * asks for a fresh preview first.
 */
import { useState } from "react";
import { errorMessageOf } from "../../api/client";
import type {
  DiagnosticsBundle,
  DiagnosticsItem,
  DiagnosticsManifest,
  DiagnosticsOptions,
  LocalizedMessage,
  MessageId,
} from "../../api/generated";
import { useLocale, useT } from "../../i18n/I18nProvider";
import type { DialogProps } from "../../layout/dialogRegistry";
import { useClient, useMessage } from "../../state/queries";
import { useUi, useUiStoreApi } from "../../state/uiStore";
import { Check, CheckGroup, WizardButtons } from "./Fields";
import { Problem } from "./Job";
import {
  DIAGNOSTICS_FILTERS,
  DIAGNOSTICS_ITEMS,
  MAX_DIAGNOSTICS_MESSAGES,
  defaultDiagnosticsOptions,
  diagnosticsIsMail,
  diagnosticsIssueId,
  diagnosticsIssues,
  diagnosticsItemId,
  formatBytes,
  isAbsolutePath,
  plainText,
  redactionId,
} from "./model";
import { DIAGNOSTICS_DIALOG } from "./names";
import { PathField, useSavePath } from "./PathField";
import { usePortabilityState } from "./store";

/** `nail_api::diagnostics::DIAGNOSTICS_MANIFEST_ENTRY`. */
const MANIFEST_ENTRY = "manifest.json";

/** A chosen message by its subject. */
function MessageLabel({ id }: { id: MessageId }) {
  const t = useT();
  const message = useMessage(id);
  const subject = message.data?.headers.subject;
  return (
    <span data-part="form.check.label">
      {subject
        ? t("portability-diagnostics-message", { subject })
        : t("portability-diagnostics-message-loading")}
    </span>
  );
}

/** Exactly what the bundle holds: every file with what was removed. */
export function DiagnosticsPreview({ manifest }: { manifest: DiagnosticsManifest }) {
  const t = useT();
  const locale = useLocale();
  const total = manifest.entries.reduce((n, e) => n + e.size_bytes, 0);
  return (
    <section
      data-part="diagnostics.preview"
      aria-label={t("portability-diagnostics-preview-title")}
    >
      <h3 data-part="portability.section.title">{t("portability-diagnostics-preview-title")}</h3>
      <p data-part="diagnostics.preview.summary" role="status">
        {t("portability-diagnostics-preview-summary", {
          count: manifest.entries.length + 1,
          size: formatBytes(total, locale),
        })}
      </p>
      <ul data-part="diagnostics.entries">
        <li data-part="diagnostics.entry" data-item="manifest">
          <span data-part="diagnostics.entry.name">{MANIFEST_ENTRY}</span>
          <span data-part="diagnostics.entry.item">{t("portability-diagnostics-manifest")}</span>
        </li>
        {manifest.entries.map((entry) => (
          <li key={entry.name} data-part="diagnostics.entry" data-item={entry.item}>
            <span data-part="diagnostics.entry.name">{entry.name}</span>
            <span data-part="diagnostics.entry.item">{t(diagnosticsItemId(entry.item))}</span>
            <span data-part="diagnostics.entry.size">{formatBytes(entry.size_bytes, locale)}</span>
            {entry.redactions.length > 0 && (
              <ul
                data-part="diagnostics.entry.redactions"
                aria-label={t("portability-diagnostics-redactions")}
              >
                {entry.redactions.map((r) => (
                  <li key={r} data-part="diagnostics.entry.redaction" data-redaction={r}>
                    {t(redactionId(r))}
                  </li>
                ))}
              </ul>
            )}
            {entry.preview !== null && (
              <details data-part="diagnostics.entry.details">
                <summary data-part="diagnostics.entry.summary">
                  {t("portability-diagnostics-show-text")}
                </summary>
                <pre data-part="diagnostics.entry.text">{entry.preview}</pre>
              </details>
            )}
          </li>
        ))}
      </ul>
    </section>
  );
}

/** The diagnostics dialog's content. */
export function DiagnosticsBundleView({ close }: { close?: () => void }) {
  const t = useT();
  const locale = useLocale();
  const client = useClient();
  const selected = useUi((s) => s.selectedMessageId);
  const offered = usePortabilityState((s) => s.diagnosticsMessages);
  const [options, setOptions] = useState<DiagnosticsOptions>(() => ({
    ...defaultDiagnosticsOptions(),
    message_ids: [],
  }));
  const [preview, setPreview] = useState<DiagnosticsManifest | null>(null);
  // `overwrite`: the Save dialog confirmed replacing an existing file.
  const { path, overwrite, onChange: setPath } = useSavePath();
  const [bundle, setBundle] = useState<DiagnosticsBundle | null>(null);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const issues = diagnosticsIssues(options);
  const mail = options.items.some(diagnosticsIsMail);
  const candidates: MessageId[] = [
    ...offered,
    ...(selected !== null && !offered.includes(selected) ? [selected] : []),
  ];

  const change = (next: DiagnosticsOptions) => {
    setOptions(next);
    // The user must see what the new options include before saving.
    setPreview(null);
    setBundle(null);
  };

  const guarded = async (work: () => Promise<void>) => {
    setBusy(true);
    setProblem(null);
    try {
      await work();
    } catch (error) {
      setProblem(errorMessageOf(error));
    } finally {
      setBusy(false);
    }
  };

  const showPreview = () =>
    guarded(async () => {
      const answer = await client.request(
        { type: "PreviewDiagnostics", options },
        "DiagnosticsPreview",
      );
      setPreview(answer.manifest);
    });

  const save = () =>
    guarded(async () => {
      const answer = await client.request(
        { type: "CreateDiagnosticsBundle", options, path: path.trim(), overwrite },
        "DiagnosticsBundle",
      );
      setBundle(answer.bundle);
    });

  return (
    <div data-part="diagnostics" data-state={bundle ? "saved" : preview ? "previewed" : "options"}>
      <p data-part="wizard.intro">{t("portability-diagnostics-intro")}</p>
      <CheckGroup<DiagnosticsItem>
        legend={t("portability-diagnostics-items")}
        field="items"
        selected={options.items}
        onChange={(items) => change({ ...options, items })}
        choices={DIAGNOSTICS_ITEMS.map((item) => ({
          value: item,
          label: t(diagnosticsItemId(item)),
          hint: t(`portability-diagnostics-item-hint-${item}`),
        }))}
      />
      {mail && (
        <fieldset data-part="form.section" data-field="messages">
          <legend data-part="form.section.title">{t("portability-diagnostics-messages")}</legend>
          <p data-part="form.hint" data-tone="warning">
            {t("portability-diagnostics-mail-warning", { max: MAX_DIAGNOSTICS_MESSAGES })}
          </p>
          {candidates.length === 0 ? (
            <p data-part="form.hint">{t("portability-diagnostics-select-message")}</p>
          ) : (
            <ul data-part="portability.choices">
              {candidates.map((id) => (
                <li key={id} data-part="portability.choice">
                  <label data-part="form.check">
                    <input
                      data-part="form.checkbox"
                      type="checkbox"
                      checked={options.message_ids.includes(id)}
                      onChange={(e) =>
                        change({
                          ...options,
                          message_ids: e.target.checked
                            ? [...options.message_ids, id]
                            : options.message_ids.filter((m) => m !== id),
                        })
                      }
                    />
                    <MessageLabel id={id} />
                  </label>
                </li>
              ))}
            </ul>
          )}
        </fieldset>
      )}
      <Check
        label={t("portability-diagnostics-redact")}
        field="redact-addresses"
        checked={options.redact_addresses}
        onChange={(redact_addresses) => change({ ...options, redact_addresses })}
        hint={t("portability-diagnostics-redact-hint")}
      />
      <p data-part="form.hint" data-field="secrets">
        {t("portability-diagnostics-secrets")}
      </p>
      {issues.length > 0 && (
        <ul data-part="diagnostics.issues">
          {issues.map((issue) => (
            <li key={issue} data-part="form.hint" data-tone="danger">
              {t(diagnosticsIssueId(issue))}
            </li>
          ))}
        </ul>
      )}
      <WizardButtons>
        <button
          type="button"
          data-part="form.button"
          data-variant={preview ? undefined : "primary"}
          data-action="preview"
          disabled={busy || issues.length > 0}
          onClick={() => void showPreview()}
        >
          {t("portability-diagnostics-preview")}
        </button>
      </WizardButtons>
      {preview && <DiagnosticsPreview manifest={preview} />}
      {preview && !bundle && (
        <>
          <PathField
            labelId="portability-diagnostics-file"
            field="bundle-path"
            value={path}
            onChange={setPath}
            kind="save-file"
            titleId="portability-diagnostics-pick-title"
            filters={DIAGNOSTICS_FILTERS}
            defaultPath={`${plainText(t("portability-diagnostics-default-name", { date: new Date().toISOString().slice(0, 10) }))}.zip`}
          />
          <WizardButtons>
            <button
              type="button"
              data-part="form.button"
              data-variant="primary"
              data-action="save"
              disabled={busy || !isAbsolutePath(path)}
              onClick={() => void save()}
            >
              {t("portability-diagnostics-save")}
            </button>
          </WizardButtons>
        </>
      )}
      {bundle && (
        <section data-part="diagnostics.saved" role="status">
          <p data-part="diagnostics.saved.text">
            {t("portability-diagnostics-saved", {
              path: bundle.path,
              size: formatBytes(bundle.size_bytes, locale),
            })}
          </p>
          <p data-part="diagnostics.saved.hash">
            {t("portability-diagnostics-sha256", { hash: bundle.sha256 })}
          </p>
          <p data-part="form.hint">{t("portability-diagnostics-saved-hint")}</p>
        </section>
      )}
      {problem && <Problem problem={problem} />}
      {close && (
        <WizardButtons>
          <button type="button" data-part="form.button" data-action="close" onClick={close}>
            {t("portability-close")}
          </button>
        </WizardButtons>
      )}
    </div>
  );
}

export function DiagnosticsDialog({ close }: DialogProps) {
  return <DiagnosticsBundleView close={close} />;
}

/** Widget `diagnostics`: what the bundle is for and the button. */
export function DiagnosticsWidget() {
  const t = useT();
  const ui = useUiStoreApi();
  return (
    <section data-part="diagnostics-settings" data-widget="diagnostics">
      <h3 data-part="form.section.title">{t("portability-diagnostics-title")}</h3>
      <p data-part="form.hint">{t("portability-diagnostics-widget-text")}</p>
      <button
        type="button"
        data-part="form.button"
        data-action="open-diagnostics"
        onClick={() => ui.getState().openWindow(DIAGNOSTICS_DIALOG)}
      >
        {t("portability-diagnostics-open")}
      </button>
    </section>
  );
}
