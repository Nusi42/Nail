/**
 * The composer widget (layout kind `composer`): identity, recipients with
 * autocomplete, subject, and the body in one of three modes — plain text,
 * HTML (TipTap) or Markdown (CodeMirror + the core's preview) — with
 * attachments, inline images, the attachment reminder, the lossy-switch
 * warning and draft autosave.
 */
import {
  useCallback,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type DragEvent,
  type SubmitEvent,
  type KeyboardEvent,
} from "react";
import { contextsOf, isFieldCombo } from "../../actions/ActionsProvider";
import { KeyMatcher, comboFromEvent } from "../../actions/keys";
import { COMPOSER_MODE_VALUES, type ComposerMode, type Identity } from "../../api/generated";
import { useLabel, useLocale, useT } from "../../i18n/I18nProvider";
import { useLayoutConfig } from "../../layout/LayoutContext";
import { optionBool, type WidgetProps } from "../../layout/widgetRegistry";
import { parseAddressList } from "../../state/addresses";
import type { ComposeContext } from "../../state/compose";
import { useComposeContext } from "../../state/hooks";
import { useChosenIdentity, useIdentities } from "../../state/queries";
import { useStore } from "zustand";
import { useUi, useUiStoreApi, type ComposerState, type UiState } from "../../state/uiStore";
import { Icon } from "../../widgets/Icon";
import { composerCryptoStore } from "../security";
import { AddressField } from "./AddressField";
import { bodyOf, draftKey } from "./draft";
import {
  cancelSwitch,
  confirmSwitch,
  requestDiscard,
  removeAttachment,
  saveRich,
  sendRich,
  swapSignature,
  switchMode,
  uploadFiles,
} from "./flows";
import { LazyHtmlEditor, LazyMarkdownEditor, prefetchEditors } from "./lazyEditors";
import { ComposerPlugins, PluginBlocks } from "./pluginExtensions";
import { ProgramDraftNotice } from "../system/ProgramDraft";
import { Preview } from "./Preview";
import { useComposerWindowOf } from "./windowOf";
import { useComposerPopOut } from "./popOut";
import { needsReminder, parseKeywords } from "./reminder";
import { composerOf, composerSessions, cryptoFor, patchComposer } from "./sessions";
import {
  autosaveDelay,
  ComposerWindowContext,
  richStoreFor,
  useComposerWindow,
  useRich,
  useRichStoreApi,
  type RichState,
} from "./store";
export { COMPOSER_MODE_VALUES as MODES } from "../../api/generated";

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
function useAutosave(
  ctx: ComposeContext,
  window: string,
  composer: ComposerState | null,
  rich: RichState,
): void {
  // The focused composer's sign / encrypt choice is part of its draft.
  useStore(composerCryptoStore, (s) => s.choice);
  const key = composer ? draftKey(composer, rich, cryptoFor(ctx.store, window)) : null;
  const busy = composer?.busy ?? null;
  const savedKey = rich.savedKey;
  useEffect(() => {
    if (key === null) return;
    const store = richStoreFor(ctx.store, window);
    if (savedKey === null) {
      // What the composer opened with is the baseline, not a change.
      store.setState({ savedKey: key });
      return;
    }
    if (key === savedKey || busy !== null) return;
    const timer = setTimeout(() => void saveRich(ctx, { quiet: true, window }), autosaveDelay());
    return () => {
      clearTimeout(timer);
    };
  }, [ctx, window, key, savedKey, busy]);
}

export { useComposerWindowOf };

/**
 * The title of a composer window (`windowTitles.composer`): the subject of
 * its message, `null` while it has none (the title is then "New message").
 */
export function composerSubjectTitle(
  state: Pick<UiState, "composer" | "composers">,
  id: string,
): string | null {
  const subject = composerOf(state, id)?.subject.trim() ?? "";
  return subject === "" ? null : subject;
}

/** `composerSubjectTitle` as text, with "New message" for no subject. */
export function ComposerWindowTitle({ id }: { id: string }) {
  const t = useT();
  const title = useUi((s) => composerSubjectTitle(s, id));
  return <>{title ?? t("window-composer")}</>;
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
  onChange: (value: string) => void;
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
        onChange={(event) => {
          onChange(event.target.value);
        }}
      />
    </div>
  );
}

function ModeSwitcher({ ctx, mode }: { ctx: ComposeContext; mode: ComposerMode }) {
  const t = useT();
  const window = useComposerWindow();
  const switching = useRich((s) => s.switching);
  return (
    <div data-part="composer.modes" role="radiogroup" aria-label={t("composer-mode-label")}>
      {COMPOSER_MODE_VALUES.map((m) => (
        <button
          key={m}
          type="button"
          role="radio"
          data-part="composer.mode"
          data-mode={m}
          data-state={m === mode ? "selected" : undefined}
          aria-checked={m === mode}
          disabled={switching}
          onClick={() => void switchMode(ctx, m, window)}
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
  const window = useComposerWindow();
  const attachments = useRich((s) => s.attachments);
  const uploads = useRich((s) => s.uploads);
  if (attachments.length === 0 && uploads === 0) return null;
  return (
    <div data-part="composer.attachments">
      <ul data-part="composer.attachments.list" aria-label={t("composer-attachments-label")}>
        {attachments.map((a, index) => (
          <li
            key={`${a.blob}-${String(index)}`}
            data-part="composer.attachment"
            data-state={a.content_id !== null ? "inline" : undefined}
          >
            <Icon name="paperclip" part="composer.attachment.icon" />
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
              onClick={() => {
                removeAttachment(ctx, index, window);
              }}
            >
              <Icon name="x" part="form.button.icon" />
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

/**
 * The composer (see the module comment). In a `composer:<key>` window it
 * edits that window's message (M5c); elsewhere the plain composer's.
 */
export function Composer(props: WidgetProps) {
  const window = useComposerWindowOf();
  const store = useUiStoreApi();
  // The focused-composer mirror and the close guard (`./sessions.ts`).
  useEffect(() => void composerSessions(store), [store]);
  return (
    <ComposerWindowContext.Provider value={window}>
      <ComposerForm {...props} window={window} />
    </ComposerWindowContext.Provider>
  );
}

function ComposerForm({ node, window }: WidgetProps & { window: string }) {
  const t = useT();
  const label = useLabel();
  const ctx = useComposeContext();
  const ids = useId();
  const layout = useLayoutConfig();
  const composer = useUi((s) => composerOf(s, window));
  const updateComposer = useCallback(
    (patch: Partial<ComposerState>) => {
      patchComposer(ctx.store, window, patch);
    },
    [ctx.store, window],
  );
  const accountId = useUi((s) => s.activeAccountId);
  const richApi = useRichStoreApi();
  const rich = useRich((s) => s);
  const identities = useIdentities(null);
  const [later, setLater] = useState(false);
  const [sendAt, setSendAt] = useState("");
  const fileInput = useRef<HTMLInputElement>(null);
  const bodyRef = useRef<HTMLTextAreaElement>(null);
  const handledAttach = useRef(rich.attachRequest);
  const allowLater = optionBool(node, "send-later", true);
  const keywords = useMemo(() => parseKeywords(t("composer-attachment-keywords")), [t]);

  useAutosave(ctx, window, composer, rich);

  // The rich editors live in their own chunks: start loading them now.
  useEffect(() => {
    prefetchEditors();
  }, []);

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
      swapSignature(ctx, from, to, window);
    }
  }, [picked, chosen.data, currentIdentity, updateComposer, identityList, ctx, window]);
  // A pop-out page opens the draft it was popped out with (M5c).
  useComposerPopOut(ctx, window, composer !== null, composer?.subject ?? null);

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

  const send = (force = false) =>
    void sendRich(ctx, later ? localToIso(sendAt) : null, { window, force });

  const submit = (event: SubmitEvent) => {
    event.preventDefault();
    send();
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
      send();
    } else if (result.match.action === "composer.save") {
      event.preventDefault();
      void saveRich(ctx, { window });
    }
  };

  const addFiles = (files: File[]) => {
    if (files.length > 0) void uploadFiles(ctx, files, false, window);
  };

  const onDrop = (event: DragEvent<HTMLFormElement>) => {
    const files = [...event.dataTransfer.files];
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
      window,
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
    // eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions -- the composer's shortcuts (send, save) and file drops arrive from the fields inside the form.
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
        if (event.dataTransfer.types.includes("Files")) event.preventDefault();
      }}
      onDrop={onDrop}
    >
      <ProgramDraftNotice />
      <div data-part="composer.field" data-field="identity">
        <label data-part="composer.field.label" htmlFor={`${ids}-identity`}>
          {t("composer-field-from")}
        </label>
        <select
          id={`${ids}-identity`}
          data-part="composer.identity"
          value={composer.identityId ?? ""}
          onChange={(event) => {
            changeIdentity(event.target.value);
          }}
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
        onChange={(to) => {
          updateComposer({ to, error: null });
        }}
      />
      {composer.showCc && (
        <AddressField
          field="cc"
          value={composer.cc}
          onChange={(cc) => {
            updateComposer({ cc, error: null });
          }}
        />
      )}
      {composer.showBcc && (
        <AddressField
          field="bcc"
          value={composer.bcc}
          onChange={(bcc) => {
            updateComposer({ bcc, error: null });
          }}
        />
      )}
      {(!composer.showCc || !composer.showBcc) && (
        <div data-part="composer.extra-fields">
          {!composer.showCc && (
            <button
              type="button"
              data-part="form.button"
              data-action="add-field"
              onClick={() => {
                updateComposer({ showCc: true });
              }}
            >
              {t("composer-add-cc")}
            </button>
          )}
          {!composer.showBcc && (
            <button
              type="button"
              data-part="form.button"
              data-action="add-field"
              onClick={() => {
                updateComposer({ showBcc: true });
              }}
            >
              {t("composer-add-bcc")}
            </button>
          )}
        </div>
      )}
      <TextField
        field="subject"
        value={composer.subject}
        onChange={(subject) => {
          updateComposer({ subject, error: null });
        }}
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
              onClick={() => {
                richApi.setState({ preview: !rich.preview });
              }}
            >
              {t("composer-preview-toggle")}
            </button>
            <label data-part="composer.option">
              <input
                type="checkbox"
                data-part="composer.option.checkbox"
                checked={rich.markdownPart}
                onChange={(event) => {
                  richApi.setState({ markdownPart: event.target.checked });
                }}
              />
              <span data-part="composer.option.label">{t("composer-markdown-part")}</span>
            </label>
          </>
        )}
        <ComposerPlugins ctx={ctx} mode={mode} textarea={bodyRef} />
        <span data-part="composer.spacer" />
        <button
          type="button"
          data-part="form.button"
          data-action="attach"
          onClick={() => fileInput.current?.click()}
        >
          <Icon name="paperclip" part="form.button.icon" />
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
            onClick={() => {
              confirmSwitch(ctx, window);
            }}
          >
            {t("composer-lossy-confirm")}
          </button>
          <button
            type="button"
            data-part="form.button"
            data-action="cancel-switch"
            onClick={() => {
              cancelSwitch(ctx, window);
            }}
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
            ref={bodyRef}
            id={`${ids}-body`}
            data-part="composer.body"
            value={composer.text}
            onChange={(event) => {
              updateComposer({ text: event.target.value, error: null });
            }}
          />
        </>
      )}
      {mode === "html" && (
        <LazyHtmlEditor
          value={rich.html}
          label={bodyLabel}
          previews={rich.inlinePreviews}
          onChange={(html) => {
            richApi.setState({ html });
          }}
          onInsertImages={(files) => uploadFiles(ctx, files, true, window)}
          source={rich.htmlSource}
          onToggleSource={() => {
            richApi.setState({ htmlSource: !rich.htmlSource });
          }}
        />
      )}
      {mode === "markdown" && (
        <div data-part="composer.markdown" data-state={rich.preview ? "split" : undefined}>
          <LazyMarkdownEditor
            value={rich.markdown}
            label={bodyLabel}
            onChange={(markdown) => {
              richApi.setState({ markdown });
            }}
          />
          {rich.preview && <Preview markdown={rich.markdown} identityId={composer.identityId} />}
          {rich.preview && <PluginBlocks markdown={rich.markdown} />}
        </div>
      )}

      <Attachments ctx={ctx} />

      {/* What sending needs (the attachment reminder, an error, the send
          time) stays with Send in the footer, which sticks to the bottom of
          a short window: nothing about sending scrolls under it. */}
      <div data-part="composer.footer">
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
                  send(true);
                }}
              >
                {t("composer-reminder-send-anyway")}
              </button>
            ) : (
              <button
                type="button"
                data-part="form.button"
                data-action="dismiss-reminder"
                onClick={() => {
                  richApi.setState({ reminder: "dismissed" });
                }}
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
              onChange={(event) => {
                setSendAt(event.target.value);
              }}
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
            <Icon name="send" part="form.button.icon" />
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
              onClick={() => {
                setLater(!later);
              }}
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
            onClick={() => void saveRich(ctx, { window })}
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
            onClick={() => void requestDiscard(ctx, window)}
          >
            {t("composer-discard")}
          </button>
        </div>
      </div>
    </form>
  );
}
