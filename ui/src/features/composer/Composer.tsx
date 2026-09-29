/**
 * The composer widget (layout kind `composer`): identity, recipients with
 * autocomplete, subject, and the body in one of three modes — plain text,
 * HTML (TipTap) or Markdown (CodeMirror + the core's preview) — with
 * attachments, inline images, the attachment reminder, the lossy-switch
 * warning and draft autosave.
 */
import {
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type DragEvent,
  type FormEvent,
  type KeyboardEvent,
} from "react";
import { contextsOf, isFieldCombo } from "../../actions/ActionsProvider";
import { KeyMatcher, comboFromEvent } from "../../actions/keys";
import type { ComposerMode, Identity } from "../../api/generated";
import { useLabel, useLocale, useT } from "../../i18n/I18nProvider";
import { useLayoutConfig } from "../../layout/LayoutContext";
import { optionBool, type WidgetProps } from "../../layout/widgetRegistry";
import { parseAddressList } from "../../state/addresses";
import type { ComposeContext } from "../../state/compose";
import { useComposeContext } from "../../state/hooks";
import { useChosenIdentity, useIdentities } from "../../state/queries";
import { useUi, type ComposerState } from "../../state/uiStore";
import { Icon } from "../../widgets/Icon";
import { AddressField } from "./AddressField";
import { bodyOf, draftKey } from "./draft";
import {
  cancelSwitch,
  confirmSwitch,
  discardRich,
  removeAttachment,
  saveRich,
  sendRich,
  swapSignature,
  switchMode,
  uploadFiles,
} from "./flows";
import { HtmlEditor } from "./HtmlEditor";
import { MarkdownEditor } from "./MarkdownEditor";
import { Preview } from "./Preview";
import { needsReminder, parseKeywords } from "./reminder";
import { AUTOSAVE_MS, richStoreFor, useRich, useRichStoreApi, type RichState } from "./store";

/** The editing modes, in switcher order. */
export const MODES: readonly ComposerMode[] = ["plain", "html", "markdown"];

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

/** Saves the draft a moment after the last change (see `AUTOSAVE_MS`). */
function useAutosave(ctx: ComposeContext, composer: ComposerState | null, rich: RichState): void {
  const key = composer ? draftKey(composer, rich) : null;
  const busy = composer?.busy ?? null;
  const savedKey = rich.savedKey;
  useEffect(() => {
    if (key === null) return;
    const store = richStoreFor(ctx.store);
    if (savedKey === null) {
      // What the composer opened with is the baseline, not a change.
      store.setState({ savedKey: key });
      return;
    }
    if (key === savedKey || busy !== null) return;
    const timer = setTimeout(() => void saveRich(ctx, { quiet: true }), AUTOSAVE_MS);
    return () => clearTimeout(timer);
  }, [ctx, key, savedKey, busy]);
}

function formatSize(bytes: number, locale: string): string {
  const kb = bytes / 1024;
  return kb < 1024
    ? new Intl.NumberFormat(locale, {
        style: "unit",
        unit: "kilobyte",
        maximumFractionDigits: 0,
      }).format(Math.max(1, kb))
    : new Intl.NumberFormat(locale, {
        style: "unit",
        unit: "megabyte",
        maximumFractionDigits: 1,
      }).format(kb / 1024);
}

function TextField({
  field,
  value,
  onChange,
}: {
  field: "subject";
  value: string;
  onChange(value: string): void;
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
        value={value}
        autoComplete="off"
        spellCheck
        onChange={(event) => onChange(event.target.value)}
      />
    </div>
  );
}

function ModeSwitcher({ ctx, mode }: { ctx: ComposeContext; mode: ComposerMode }) {
  const t = useT();
  const switching = useRich((s) => s.switching);
  return (
    <div data-part="composer.modes" role="radiogroup" aria-label={t("composer-mode-label")}>
      {MODES.map((m) => (
        <button
          key={m}
          type="button"
          role="radio"
          data-part="composer.mode"
          data-mode={m}
          data-state={m === mode ? "selected" : undefined}
          aria-checked={m === mode}
          disabled={switching}
          onClick={() => void switchMode(ctx, m)}
        >
          {t(`composer-mode-${m}`)}
        </button>
      ))}
    </div>
  );
}

function Attachments({ ctx }: { ctx: ComposeContext }) {
  const t = useT();
  const locale = useLocale();
  const attachments = useRich((s) => s.attachments);
  const uploads = useRich((s) => s.uploads);
  if (attachments.length === 0 && uploads === 0) return null;
  return (
    <div data-part="composer.attachments">
      <ul data-part="composer.attachments.list" aria-label={t("composer-attachments-label")}>
        {attachments.map((a, index) => (
          <li
            key={`${a.blob}-${index}`}
            data-part="composer.attachment"
            data-state={a.content_id !== null ? "inline" : undefined}
          >
            <Icon name="attachment" part="composer.attachment.icon" />
            <span data-part="composer.attachment.name">{a.filename}</span>
            <span data-part="composer.attachment.size">{formatSize(a.size, locale)}</span>
            {a.content_id !== null && (
              <span data-part="composer.attachment.inline">{t("composer-attachment-inline")}</span>
            )}
            <button
              type="button"
              data-part="form.button"
              data-action="remove-attachment"
              aria-label={t("composer-attachment-remove", { name: a.filename })}
              onClick={() => removeAttachment(ctx, index)}
            >
              <Icon name="close" part="form.button.icon" />
              <span data-part="form.button.label">{t("composer-attachment-remove-short")}</span>
            </button>
          </li>
        ))}
      </ul>
      {uploads > 0 && (
        <span data-part="composer.uploading" aria-live="polite">
          {t("composer-uploading", { count: uploads })}
        </span>
      )}
    </div>
  );
}

/** The composer (see the module comment). */
export function Composer({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const ctx = useComposeContext();
  const ids = useId();
  const layout = useLayoutConfig();
  const composer = useUi((s) => s.composer);
  const updateComposer = useUi((s) => s.updateComposer);
  const accountId = useUi((s) => s.activeAccountId);
  const richApi = useRichStoreApi();
  const rich = useRich((s) => s);
  const identities = useIdentities(null);
  const [later, setLater] = useState(false);
  const [sendAt, setSendAt] = useState("");
  const fileInput = useRef<HTMLInputElement>(null);
  const handledAttach = useRef(rich.attachRequest);
  const allowLater = optionBool(node, "send-later", true);
  const keywords = useMemo(() => parseKeywords(t("composer-attachment-keywords")), [t]);

  useAutosave(ctx, composer, rich);

  // `composer.attach` asks for the file picker.
  useEffect(() => {
    if (rich.attachRequest !== handledAttach.current) {
      handledAttach.current = rich.attachRequest;
      fileInput.current?.click();
    }
  }, [rich.attachRequest]);

  // The core picks the identity (per-domain rules of the active virtual
  // account, the identity a reply goes out from) until the user chooses one.
  const recipients = composer ? `${composer.to},${composer.cc},${composer.bcc}` : "";
  const picked = composer?.identityPicked ?? true;
  const currentIdentity = composer?.identityId ?? null;
  const emails = useMemo(
    () => parseAddressList(recipients).addresses.map((a) => a.email),
    [recipients],
  );
  const chosen = useChosenIdentity(
    accountId,
    emails,
    composer?.context?.message_id ?? null,
    !picked,
  );
  const identityList = identities.data;
  useEffect(() => {
    if (picked || chosen.data === undefined || chosen.data === null) return;
    if (chosen.data !== currentIdentity) {
      const from = identityList?.find((i) => i.id === currentIdentity);
      const to = identityList?.find((i) => i.id === chosen.data);
      updateComposer({ identityId: chosen.data });
      swapSignature(ctx, from, to);
    }
  }, [picked, chosen.data, currentIdentity, updateComposer, identityList, ctx]);

  if (!composer) {
    return (
      <div data-part="composer" data-node-id={node.id ?? undefined} data-state="empty">
        <div data-part="composer.empty">{t("composer-empty")}</div>
      </div>
    );
  }

  const busy = composer.busy !== null;
  const mode = rich.mode;
  const reminder =
    rich.reminder !== "dismissed" &&
    needsReminder(
      mode,
      bodyOf(composer, rich),
      composer.subject,
      rich.attachments.length,
      keywords,
    );
  const bodyLabel = t("composer-field-body");

  const submit = (event: FormEvent) => {
    event.preventDefault();
    void sendRich(ctx, later ? localToIso(sendAt) : null);
  };

  // The layout's composer keys (send, save) run the rich flows here, before
  // the global dispatcher would run the plain-text ones.
  const onKeyDown = (event: KeyboardEvent<HTMLFormElement>) => {
    if (event.nativeEvent.isComposing) return;
    const combo = comboFromEvent(event.nativeEvent);
    if (!combo || !isFieldCombo(combo)) return;
    const target = event.target instanceof Element ? event.target : null;
    const result = new KeyMatcher(layout.keys).feed(combo, contextsOf(target));
    if (result.kind !== "match") return;
    if (result.match.action === "composer.send") {
      event.preventDefault();
      void sendRich(ctx, later ? localToIso(sendAt) : null);
    } else if (result.match.action === "composer.save") {
      event.preventDefault();
      void saveRich(ctx);
    }
  };

  const addFiles = (files: File[]) => {
    if (files.length > 0) void uploadFiles(ctx, files, false);
  };

  const onDrop = (event: DragEvent<HTMLFormElement>) => {
    const files = [...(event.dataTransfer?.files ?? [])];
    if (files.length === 0) return;
    event.preventDefault();
    addFiles(files);
  };

  const changeIdentity = (id: string) => {
    const list = identities.data ?? [];
    swapSignature(
      ctx,
      list.find((i) => i.id === composer.identityId),
      list.find((i) => i.id === id),
    );
    updateComposer({ identityId: id, identityPicked: true, error: null });
  };

  const savedText =
    rich.autosave === "saving"
      ? t("composer-autosave-saving")
      : rich.autosave === "failed"
        ? t("composer-autosave-failed")
        : composer.draftId !== null && composer.busy === null
          ? t("composer-saved")
          : null;

  return (
    <form
      data-part="composer"
      data-node-id={node.id ?? undefined}
      data-key-context="composer"
      data-mode={mode}
      data-state={composer.busy ?? undefined}
      aria-label={node.label ? label(node.label) : t("composer-label")}
      aria-busy={busy}
      onSubmit={submit}
      onKeyDown={onKeyDown}
      onDragOver={(event) => {
        if (event.dataTransfer?.types.includes("Files")) event.preventDefault();
      }}
      onDrop={onDrop}
    >
      <div data-part="composer.field" data-field="identity">
        <label data-part="composer.field.label" htmlFor={`${ids}-identity`}>
          {t("composer-field-from")}
        </label>
        <select
          id={`${ids}-identity`}
          data-part="composer.identity"
          value={composer.identityId ?? ""}
          onChange={(event) => changeIdentity(event.target.value)}
        >
          {composer.identityId === null && <option value="">{t("composer-no-identity")}</option>}
          {(identities.data ?? []).map((identity) => (
            <option key={identity.id} value={identity.id}>
              {identityLabel(identity)}
            </option>
          ))}
        </select>
      </div>
      <AddressField
        field="to"
        value={composer.to}
        onChange={(to) => updateComposer({ to, error: null })}
      />
      {composer.showCc && (
        <AddressField
          field="cc"
          value={composer.cc}
          onChange={(cc) => updateComposer({ cc, error: null })}
        />
      )}
      {composer.showBcc && (
        <AddressField
          field="bcc"
          value={composer.bcc}
          onChange={(bcc) => updateComposer({ bcc, error: null })}
        />
      )}
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
      <TextField
        field="subject"
        value={composer.subject}
        onChange={(subject) => updateComposer({ subject, error: null })}
      />

      <div data-part="composer.toolbar">
        <ModeSwitcher ctx={ctx} mode={mode} />
        {mode === "markdown" && (
          <>
            <button
              type="button"
              data-part="form.button"
              data-action="toggle-preview"
              aria-pressed={rich.preview}
              data-state={rich.preview ? "selected" : undefined}
              onClick={() => richApi.setState({ preview: !rich.preview })}
            >
              {t("composer-preview-toggle")}
            </button>
            <label data-part="composer.option">
              <input
                type="checkbox"
                data-part="composer.option.checkbox"
                checked={rich.markdownPart}
                onChange={(event) => richApi.setState({ markdownPart: event.target.checked })}
              />
              <span data-part="composer.option.label">{t("composer-markdown-part")}</span>
            </label>
          </>
        )}
        <span data-part="composer.spacer" />
        <button
          type="button"
          data-part="form.button"
          data-action="attach"
          onClick={() => fileInput.current?.click()}
        >
          <Icon name="attachment" part="form.button.icon" />
          <span data-part="form.button.label">{t("composer-attach")}</span>
        </button>
        <input
          ref={fileInput}
          data-part="composer.file-input"
          type="file"
          multiple
          hidden
          aria-label={t("composer-attach")}
          onChange={(event) => {
            const files = [...(event.target.files ?? [])];
            event.target.value = "";
            addFiles(files);
          }}
        />
      </div>

      {rich.pendingSwitch && (
        <div data-part="composer.lossy" role="alert">
          <span data-part="composer.lossy.message">
            {t("composer-lossy-warning", {
              from: t(`composer-mode-${mode}`),
              to: t(`composer-mode-${rich.pendingSwitch.mode}`),
            })}
          </span>
          <button
            type="button"
            data-part="form.button"
            data-action="confirm-switch"
            data-variant="primary"
            onClick={() => confirmSwitch(ctx)}
          >
            {t("composer-lossy-confirm")}
          </button>
          <button
            type="button"
            data-part="form.button"
            data-action="cancel-switch"
            onClick={() => cancelSwitch(ctx)}
          >
            {t("composer-lossy-cancel")}
          </button>
        </div>
      )}

      {mode === "plain" && (
        <>
          <label data-part="composer.body.label" htmlFor={`${ids}-body`}>
            {bodyLabel}
          </label>
          <textarea
            id={`${ids}-body`}
            data-part="composer.body"
            value={composer.text}
            onChange={(event) => updateComposer({ text: event.target.value, error: null })}
          />
        </>
      )}
      {mode === "html" && (
        <HtmlEditor
          value={rich.html}
          label={bodyLabel}
          previews={rich.inlinePreviews}
          onChange={(html) => richApi.setState({ html })}
          onInsertImages={(files) => uploadFiles(ctx, files, true)}
          source={rich.htmlSource}
          onToggleSource={() => richApi.setState({ htmlSource: !rich.htmlSource })}
        />
      )}
      {mode === "markdown" && (
        <div data-part="composer.markdown" data-state={rich.preview ? "split" : undefined}>
          <MarkdownEditor
            value={rich.markdown}
            label={bodyLabel}
            onChange={(markdown) => richApi.setState({ markdown })}
          />
          {rich.preview && <Preview markdown={rich.markdown} identityId={composer.identityId} />}
        </div>
      )}

      <Attachments ctx={ctx} />

      {reminder && (
        <div
          data-part="composer.reminder"
          data-state={rich.reminder === "confirm" ? "confirm" : undefined}
          role={rich.reminder === "confirm" ? "alert" : undefined}
        >
          <span data-part="composer.reminder.message">{t("composer-reminder")}</span>
          <button
            type="button"
            data-part="form.button"
            data-action="attach"
            onClick={() => fileInput.current?.click()}
          >
            {t("composer-attach")}
          </button>
          {rich.reminder === "confirm" ? (
            <button
              type="button"
              data-part="form.button"
              data-action="send-anyway"
              onClick={() => {
                richApi.setState({ reminder: "dismissed" });
                void sendRich(ctx, later ? localToIso(sendAt) : null, { force: true });
              }}
            >
              {t("composer-reminder-send-anyway")}
            </button>
          ) : (
            <button
              type="button"
              data-part="form.button"
              data-action="dismiss-reminder"
              onClick={() => richApi.setState({ reminder: "dismissed" })}
            >
              {t("composer-reminder-dismiss")}
            </button>
          )}
        </div>
      )}

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
          onClick={() => void saveRich(ctx)}
        >
          {t("composer-save")}
        </button>
        {savedText !== null && (
          <span data-part="composer.saved" data-state={rich.autosave} aria-live="polite">
            {savedText}
          </span>
        )}
        <button
          type="button"
          data-part="form.button"
          data-action="discard"
          disabled={busy}
          onClick={() => void discardRich(ctx)}
        >
          {t("composer-discard")}
        </button>
      </div>
    </form>
  );
}
