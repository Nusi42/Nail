/**
 * Before-send vetoes (M4a, ADR 0079). A plug-in's before-send hook may
 * stop a message with a reason; `SendDraft` then answers `SendVetoed`
 * and nothing is queued. The veto dialog always names each plug-in and
 * shows its reason, and offers "Keep editing" (the default) and "Send
 * anyway" (`OverrideSendVeto`, valid for 10 minutes; the other hooks'
 * headers and content still apply).
 *
 * Mount points: the composers call `sendDraftChecked` instead of
 * requesting `SendDraft` → `SendQueued` directly (it resolves with the
 * outbox entry, or `null` when the user keeps editing); the dialog is the
 * modal confirmation window `plugin-send-veto` (M5c: it blocks the composer
 * it came from), kept in step with the veto by the feature effect
 * `SendVetoDialog`; closing the window (× or Escape) keeps editing.
 */
import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { errorMessageOf, NailError, type NailClient } from "../../api/client";
import type { LocalizedMessage, OutboxEntry, Request } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { useClient } from "../../state/queries";
import { useUi, useUiStoreApi } from "../../state/uiStore";
import type { UiStore } from "../../state/uiStore";
import { pluginsStoreFor, usePluginsState } from "./store";
import { SEND_VETO_WINDOW, setPluginWindowTitle } from "./windows";

/**
 * Sends a draft like `SendDraft` → `SendQueued`, but when before-send hooks
 * veto it, shows the veto dialog and waits: the outbox entry once the user
 * chose "Send anyway", `null` when they keep editing. Errors throw
 * `NailError` as `NailClient.request` does.
 */
export async function sendDraftChecked(
  client: NailClient,
  store: UiStore,
  request: Extract<Request, { type: "SendDraft" }>,
): Promise<OutboxEntry | null> {
  const response = await client.call(request);
  switch (response.type) {
    case "SendQueued":
      return response.entry;
    case "SendVetoed":
      return new Promise<OutboxEntry | null>((resolve) => {
        pluginsStoreFor(store).getState().showVeto({
          draftId: response.draft_id,
          vetoId: response.veto_id,
          vetoes: response.vetoes,
          settle: resolve,
        });
      });
    case "Error":
      throw new NailError(response.code, response.message, response.localized ?? null);
    default:
      throw new NailError(
        "internal",
        `expected a SendQueued response to SendDraft, got ${response.type}`,
      );
  }
}

/**
 * Keeps the veto and its window in step (an effect in the app shell): a
 * veto opens `plugin-send-veto`, a settled one closes it, and a window the
 * user closed (× or Escape) keeps editing.
 */
export function SendVetoDialog() {
  const t = useT();
  const store = useUiStoreApi();
  const veto = usePluginsState((s) => s.veto);
  const showVeto = usePluginsState((s) => s.showVeto);
  const open = useUi((s) => s.windows.includes(SEND_VETO_WINDOW));
  const shown = useRef<string | null>(null);
  useEffect(() => {
    if (veto !== null && shown.current !== veto.vetoId) {
      shown.current = veto.vetoId;
      setPluginWindowTitle(SEND_VETO_WINDOW, t("plugin-veto-title", { count: veto.vetoes.length }));
      store.getState().openWindow(SEND_VETO_WINDOW);
    } else if (veto === null && shown.current !== null) {
      shown.current = null;
      if (store.getState().windows.includes(SEND_VETO_WINDOW)) {
        store.getState().closeWindow(SEND_VETO_WINDOW);
      }
    } else if (veto !== null && !open) {
      // Closed by the user: keep editing.
      shown.current = null;
      veto.settle(null);
      showVeto(null);
    }
  }, [veto, open, store, showVeto, t]);
  return null;
}

/** The content of the veto's window. */
export function SendVetoWindow() {
  const veto = usePluginsState((s) => s.veto);
  if (!veto) return null;
  return <VetoView key={veto.vetoId} />;
}

function VetoView() {
  const t = useT();
  const client = useClient();
  const veto = usePluginsState((s) => s.veto);
  const showVeto = usePluginsState((s) => s.showVeto);
  const ref = useRef<HTMLDivElement>(null);
  const keep = useRef<HTMLButtonElement>(null);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const [expired, setExpired] = useState(false);

  useEffect(() => {
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    keep.current?.focus();
    return () => {
      if (previous?.isConnected) previous.focus();
    };
  }, []);

  if (!veto) return null;

  const close = (entry: OutboxEntry | null) => {
    // Settle first: replacing the veto settles it as "keep editing", which
    // is then a no-op (a promise settles once).
    veto.settle(entry);
    showVeto(null);
  };

  const sendAnyway = async () => {
    setBusy(true);
    setProblem(null);
    try {
      const { entry } = await client.request(
        { type: "OverrideSendVeto", veto_id: veto.vetoId },
        "SendQueued",
      );
      close(entry);
    } catch (error) {
      if (error instanceof NailError && error.code === "not_found") setExpired(true);
      else setProblem(errorMessageOf(error));
      setBusy(false);
    }
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      if (!busy) close(null);
      return;
    }
  };

  return (
    <>
      {/* eslint-disable-next-line jsx-a11y/no-static-element-interactions -- the confirmation's keys (Enter, Escape) arrive from the buttons inside it. */}
      <div
        ref={ref}
        data-part="plugin-veto"
        data-key-context="plugin-veto"
        aria-busy={busy}
        onKeyDown={onKeyDown}
      >
        <p data-part="plugin-veto.intro">{t("plugin-veto-intro", { count: veto.vetoes.length })}</p>
        <ul data-part="plugin-veto.list">
          {veto.vetoes.map((v) => (
            <li key={v.plugin_id} data-part="plugin-veto.item" data-plugin={v.plugin_id}>
              <strong data-part="plugin-veto.plugin">
                {t("plugin-veto-plugin", { plugin: v.plugin_name })}
              </strong>
              <q data-part="plugin-veto.reason">{v.reason}</q>
            </li>
          ))}
        </ul>
        {expired && (
          <p data-part="plugin-veto.error" role="alert">
            {t("plugin-veto-expired")}
          </p>
        )}
        {problem && (
          <p data-part="plugin-veto.error" role="alert">
            {t(problem.id, problem.args)}
          </p>
        )}
        <div data-part="plugin-veto.buttons">
          <button
            ref={keep}
            type="button"
            data-part="form.button"
            data-action="keep-editing"
            data-variant="primary"
            disabled={busy}
            onClick={() => {
              close(null);
            }}
          >
            {t("common-keep-editing")}
          </button>
          {!expired && (
            <button
              type="button"
              data-part="form.button"
              data-action="send-anyway"
              data-variant="danger"
              disabled={busy}
              onClick={() => void sendAnyway()}
            >
              {t("common-send-anyway")}
            </button>
          )}
        </div>
      </div>
    </>
  );
}
