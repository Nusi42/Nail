/**
 * The Autocrypt setup message flow (Autocrypt Level 1): send the own
 * OpenPGP key to yourself, protected by a setup code that is shown once,
 * and import such a message on another device with that code.
 *
 * Layout widget `security-autocrypt` holds both halves; the dialog
 * `security-autocrypt-import` (action `security.autocrypt.import`) imports
 * the message selected in the list.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useState, type SubmitEvent } from "react";
import { errorMessageIdOf } from "../../api/client";
import type { KeyImportResult, MessageId } from "../../api/generated";
import { useLabel, useT } from "../../i18n/I18nProvider";
import type { DialogProps } from "../../layout/dialogRegistry";
import type { WidgetProps } from "../../layout/widgetRegistry";
import { useClient, useIdentities, useMessage } from "../../state/queries";
import { useUi } from "../../state/uiStore";
import { SelectField, TextField } from "../../widgets/forms";
import { identityText } from "./IdentityCrypto";
import { ImportResult } from "./KeyTools";
import { formatPasscode, isCompletePasscode } from "./model";
import { securityKeys } from "./queries";
import { nonEmpty } from "../../text/empty";

/** Name of the import dialog (`window.open "security-autocrypt-import"`). */
export const AUTOCRYPT_IMPORT_DIALOG = "security-autocrypt-import";

function SendSetup() {
  const t = useT();
  const client = useClient();
  const identities = useIdentities(null).data ?? [];
  const [identity, setIdentity] = useState("");
  const [sent, setSent] = useState<{ passcode: string } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const chosen = nonEmpty(identity) ?? identities[0]?.id ?? "";

  const send = async () => {
    if (chosen === "" || busy) return;
    setBusy(true);
    setError(null);
    try {
      const answer = await client.request(
        { type: "SendAutocryptSetup", identity_id: chosen },
        "AutocryptSetupSent",
      );
      setSent({ passcode: answer.passcode });
    } catch (e) {
      const id = errorMessageIdOf(e);
      setError(id === "error-no-key" ? "security-autocrypt-no-key" : id);
    } finally {
      setBusy(false);
    }
  };

  return (
    <section data-part="security-autocrypt.send" aria-label={t("security-autocrypt-send-title")}>
      <h3 data-part="security-autocrypt.title">{t("security-autocrypt-send-title")}</h3>
      <p data-part="security-autocrypt.intro">{t("security-autocrypt-send-intro")}</p>
      {sent ? (
        <div
          data-part="security-autocrypt.code"
          role="region"
          aria-label={t("security-autocrypt-code-title")}
        >
          <p data-part="security-autocrypt.code.title">{t("security-autocrypt-code-title")}</p>
          <p data-part="security-autocrypt.code.warning" role="alert">
            {t("security-autocrypt-code-once")}
          </p>
          <code data-part="security-autocrypt.code.value">{sent.passcode}</code>
          <p data-part="form.hint">{t("security-autocrypt-code-next")}</p>
          <button
            type="button"
            data-part="form.button"
            data-variant="primary"
            data-action="code-done"
            onClick={() => {
              setSent(null);
            }}
          >
            {t("security-autocrypt-code-done")}
          </button>
        </div>
      ) : (
        <>
          <SelectField
            labelId="security-autocrypt-identity"
            name="autocrypt-identity"
            value={chosen}
            onChange={setIdentity}
            choices={identities.map((i) => ({
              value: i.id,
              label: identityText(i),
              literal: true,
            }))}
          />
          <button
            type="button"
            data-part="form.button"
            data-variant="primary"
            data-action="send-setup"
            disabled={busy || chosen === ""}
            onClick={() => void send()}
          >
            {t("security-autocrypt-send")}
          </button>
        </>
      )}
      {error && (
        <p data-part="security-autocrypt.error" role="alert">
          {t(error)}
        </p>
      )}
    </section>
  );
}

/** Setup-code form that imports the key from one setup message. */
export function AutocryptImport({
  messageId,
  onDone,
}: {
  messageId: MessageId;
  onDone?: (result: KeyImportResult) => void;
}) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const [code, setCode] = useState("");
  const [result, setResult] = useState<KeyImportResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = async (event: SubmitEvent) => {
    event.preventDefault();
    if (!isCompletePasscode(code) || busy) return;
    setBusy(true);
    setError(null);
    try {
      const answer = await client.request(
        { type: "ImportAutocryptSetup", message_id: messageId, passcode: formatPasscode(code) },
        "KeysImported",
      );
      setResult(answer.result);
      setCode("");
      await queryClient.invalidateQueries({ queryKey: securityKeys.all });
      onDone?.(answer.result);
    } catch (e) {
      const id = errorMessageIdOf(e);
      setError(
        id === "error-auth-failed"
          ? "security-autocrypt-wrong-code"
          : id === "error-invalid-request"
            ? "security-autocrypt-not-setup"
            : id,
      );
    } finally {
      setBusy(false);
    }
  };

  if (result) return <ImportResult result={result} />;
  return (
    <form
      data-part="security-autocrypt.import"
      aria-label={t("security-autocrypt-import-title")}
      noValidate
      onSubmit={(e) => void submit(e)}
    >
      <TextField
        labelId="security-autocrypt-code"
        hintId="security-autocrypt-code-hint"
        name="autocrypt-code"
        autoComplete="one-time-code"
        value={code}
        onChange={(v) => {
          setCode(formatPasscode(v));
        }}
      />
      <button
        type="submit"
        data-part="form.button"
        data-variant="primary"
        data-action="import-setup"
        disabled={busy || !isCompletePasscode(code)}
      >
        {t("security-autocrypt-import")}
      </button>
      {error && (
        <p data-part="security-autocrypt.error" role="alert">
          {t(error)}
        </p>
      )}
    </form>
  );
}

function ImportSelected() {
  const t = useT();
  const selectedId = useUi((s) => s.selectedMessageId);
  const message = useMessage(selectedId).data;
  return (
    <section
      data-part="security-autocrypt.receive"
      aria-label={t("security-autocrypt-import-title")}
    >
      <h3 data-part="security-autocrypt.title">{t("security-autocrypt-import-title")}</h3>
      <p data-part="security-autocrypt.intro">{t("security-autocrypt-import-intro")}</p>
      {selectedId === null || !message ? (
        <p data-part="security-autocrypt.hint">{t("security-autocrypt-select-message")}</p>
      ) : (
        <>
          <p data-part="security-autocrypt.message">
            {t("security-autocrypt-selected", {
              subject: message.headers.subject || t("security-autocrypt-no-subject"),
            })}
          </p>
          <AutocryptImport key={selectedId} messageId={selectedId} />
        </>
      )}
    </section>
  );
}

/** Layout widget `security-autocrypt`. */
export function AutocryptSetup({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  return (
    <section
      data-part="security-autocrypt"
      data-node-id={node.id ?? undefined}
      aria-label={node.label ? label(node.label) : t("security-autocrypt-label")}
    >
      <SendSetup />
      <ImportSelected />
    </section>
  );
}

/** Dialog `security-autocrypt-import`: imports the selected setup message. */
export function AutocryptImportDialog({ close }: DialogProps) {
  const t = useT();
  const selectedId = useUi((s) => s.selectedMessageId);
  const [done, setDone] = useState(false);
  return (
    <div data-part="security-autocrypt.dialog">
      <p data-part="security-autocrypt.intro">{t("security-autocrypt-import-intro")}</p>
      {selectedId === null ? (
        <p data-part="security-autocrypt.hint">{t("security-autocrypt-select-message")}</p>
      ) : (
        <AutocryptImport
          messageId={selectedId}
          onDone={() => {
            setDone(true);
          }}
        />
      )}
      <button type="button" data-part="form.button" data-action="close" onClick={close}>
        {t(done ? "security-autocrypt-code-done" : "security-cancel")}
      </button>
    </div>
  );
}
