import { useEffect, useId, useMemo, useState, type FormEvent } from "react";
import type { Identity } from "../api/generated";
import { useLabel, useT } from "../i18n/I18nProvider";
import { optionBool, type WidgetProps } from "../layout/widgetRegistry";
import { parseAddressList } from "../state/addresses";
import { discardComposer, saveComposer, sendComposer } from "../state/compose";
import { useComposeContext } from "../state/hooks";
import { useChosenIdentity, useIdentities } from "../state/queries";
import { useUi, type ComposerState } from "../state/uiStore";
import { Icon } from "./Icon";

/** `Ada Lovelace <ada@example.org>` for the identity picker. */
export function identityLabel(identity: Identity): string {
  return identity.name ? `${identity.name} <${identity.email}>` : identity.email;
}

/** `datetime-local` value → RFC 3339 (UTC), or `null` when empty / invalid. */
export function localToIso(value: string): string | null {
  if (value.trim() === "") return null;
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? null : date.toISOString();
}

type TextField = "to" | "cc" | "bcc" | "subject";

function Field({
  field,
  composer,
  onChange,
}: {
  field: TextField;
  composer: ComposerState;
  onChange: (field: TextField, value: string) => void;
}) {
  const t = useT();
  const id = useId();
  return (
    <div data-part="composer.field" data-field={field}>
      <label data-part="composer.field.label" htmlFor={id}>
        {t(`composer-field-${field}`)}
      </label>
      <input
        id={id}
        data-part="composer.field.input"
        type="text"
        value={composer[field]}
        autoComplete="off"
        spellCheck={field === "subject"}
        onChange={(event) => onChange(field, event.target.value)}
      />
    </div>
  );
}

/**
 * The plain-text composer (M1a): identity, recipients, subject, body; send
 * with undo-send, send later, save, discard. The message lives in the UI
 * state until it is saved or sent through the core.
 */
export function Composer({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const ctx = useComposeContext();
  const ids = useId();
  const composer = useUi((s) => s.composer);
  const updateComposer = useUi((s) => s.updateComposer);
  const accountId = useUi((s) => s.activeAccountId);
  const identities = useIdentities(null);
  const [later, setLater] = useState(false);
  const [sendAt, setSendAt] = useState("");
  const allowLater = optionBool(node, "send-later", true);

  // The core picks the identity (per-domain rules of the active virtual
  // account, the identity a reply goes out from) until the user chooses one.
  const recipients = composer ? `${composer.to},${composer.cc},${composer.bcc}` : "";
  const picked = composer?.identityPicked ?? true;
  const currentIdentity = composer?.identityId ?? null;
  const emails = useMemo(
    () => parseAddressList(recipients).addresses.map((a) => a.email),
    [recipients],
  );
  const chosen = useChosenIdentity(accountId, emails, composer?.inReplyTo ?? null, !picked);
  useEffect(() => {
    if (picked || chosen.data === undefined || chosen.data === null) return;
    if (chosen.data !== currentIdentity) updateComposer({ identityId: chosen.data });
  }, [picked, chosen.data, currentIdentity, updateComposer]);

  if (!composer) {
    return (
      <div data-part="composer" data-node-id={node.id ?? undefined} data-state="empty">
        <div data-part="composer.empty">{t("composer-empty")}</div>
      </div>
    );
  }

  const onChange = (field: TextField, value: string) =>
    updateComposer({ [field]: value, error: null });
  const busy = composer.busy !== null;
  const submit = (event: FormEvent) => {
    event.preventDefault();
    void sendComposer(ctx, later ? localToIso(sendAt) : null);
  };

  return (
    <form
      data-part="composer"
      data-node-id={node.id ?? undefined}
      data-key-context="composer"
      data-state={composer.busy ?? undefined}
      aria-label={node.label ? label(node.label) : t("composer-label")}
      aria-busy={busy}
      onSubmit={submit}
    >
      <div data-part="composer.field" data-field="identity">
        <label data-part="composer.field.label" htmlFor={`${ids}-identity`}>
          {t("composer-field-from")}
        </label>
        <select
          id={`${ids}-identity`}
          data-part="composer.identity"
          value={composer.identityId ?? ""}
          onChange={(event) =>
            updateComposer({ identityId: event.target.value, identityPicked: true, error: null })
          }
        >
          {composer.identityId === null && <option value="">{t("composer-no-identity")}</option>}
          {(identities.data ?? []).map((identity) => (
            <option key={identity.id} value={identity.id}>
              {identityLabel(identity)}
            </option>
          ))}
        </select>
      </div>
      <Field field="to" composer={composer} onChange={onChange} />
      {composer.showCc && <Field field="cc" composer={composer} onChange={onChange} />}
      {composer.showBcc && <Field field="bcc" composer={composer} onChange={onChange} />}
      {(!composer.showCc || !composer.showBcc) && (
        <div data-part="composer.extra-fields">
          {!composer.showCc && (
            <button
              type="button"
              data-part="form.button"
              data-action="add-field"
              onClick={() => updateComposer({ showCc: true })}
            >
              {t("composer-add-cc")}
            </button>
          )}
          {!composer.showBcc && (
            <button
              type="button"
              data-part="form.button"
              data-action="add-field"
              onClick={() => updateComposer({ showBcc: true })}
            >
              {t("composer-add-bcc")}
            </button>
          )}
        </div>
      )}
      <Field field="subject" composer={composer} onChange={onChange} />
      <label data-part="composer.body.label" htmlFor={`${ids}-body`}>
        {t("composer-field-body")}
      </label>
      <textarea
        id={`${ids}-body`}
        data-part="composer.body"
        value={composer.text}
        onChange={(event) => updateComposer({ text: event.target.value, error: null })}
      />
      {composer.error && (
        <div data-part="composer.error" role="alert">
          {t(composer.error.id, composer.error.args)}
        </div>
      )}
      {later && (
        <div data-part="composer.send-later">
          <label data-part="composer.field.label" htmlFor={`${ids}-send-at`}>
            {t("composer-send-at")}
          </label>
          <input
            id={`${ids}-send-at`}
            data-part="composer.send-at"
            type="datetime-local"
            value={sendAt}
            onChange={(event) => setSendAt(event.target.value)}
          />
        </div>
      )}
      <div data-part="composer.actions">
        <button
          type="submit"
          data-part="form.button"
          data-action="send"
          data-variant="primary"
          disabled={busy}
        >
          <Icon name="sent" part="form.button.icon" />
          <span data-part="form.button.label">
            {t(later ? "composer-schedule" : "composer-send")}
          </span>
        </button>
        {allowLater && (
          <button
            type="button"
            data-part="form.button"
            data-action="send-later"
            aria-pressed={later}
            data-state={later ? "selected" : undefined}
            disabled={busy}
            onClick={() => setLater(!later)}
          >
            {t("composer-send-later")}
          </button>
        )}
        <span data-part="composer.spacer" />
        <button
          type="button"
          data-part="form.button"
          data-action="save"
          disabled={busy}
          onClick={() => void saveComposer(ctx)}
        >
          {t("composer-save")}
        </button>
        {composer.draftId !== null && composer.busy === null && (
          <span data-part="composer.saved">{t("composer-saved")}</span>
        )}
        <button
          type="button"
          data-part="form.button"
          data-action="discard"
          disabled={busy}
          onClick={() => void discardComposer(ctx)}
        >
          {t("composer-discard")}
        </button>
      </div>
    </form>
  );
}
