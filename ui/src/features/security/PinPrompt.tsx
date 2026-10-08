/**
 * Smartcard PIN prompts (M4c WP124; contract ADR 0094): when a backend
 * needs a card's PIN the core sends `PinRequested`; the dialog
 * `smartcard-pin` opens with the card, what the PIN is for (unlock, sign,
 * decrypt), the key, the attempts left and — after a wrong PIN — that it
 * was wrong. The PIN goes to the core once (`AnswerPinPrompt`) and is
 * forgotten; Cancel (or closing the dialog) answers `null`. When the core
 * stops waiting (`PinPromptClosed`: answered elsewhere, timed out, card
 * removed) the dialog says why instead of asking. PINs are never stored,
 * logged or shown; remote sessions never get these events.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useId, useRef, useState, type SubmitEvent } from "react";
import { useStore } from "zustand";
import { createStore, type StoreApi } from "zustand/vanilla";
import {
  PIN_PROMPT_CLOSE_REASON_MESSAGE_IDS,
  PIN_PURPOSE_MESSAGE_IDS,
  type PinPrompt,
  type PinPromptCloseReason,
} from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { useDateFormat } from "../../i18n/format";
import type { DialogProps } from "../../layout/dialogRegistry";
import { useClient } from "../../state/queries";
import { useUiStoreApi, type UiStore } from "../../state/uiStore";
import type { FeatureEventHandler } from "../types";
import { pinIsPlausible } from "./smartcardModel";
import { smartcardKeys } from "./Smartcards";
import { useBusyAction } from "../../state/busyAction";

export const PIN_DIALOG = "smartcard-pin";

export interface PinUiState {
  /** Prompts waiting for an answer, oldest first. */
  prompts: PinPrompt[];
  /** Why the shown prompt closed without this window's answer. */
  closed: PinPromptCloseReason | null;
  request: (prompt: PinPrompt) => void;
  /** The core closed a prompt; `shown` = it was the one on screen. */
  close: (promptId: string, reason: PinPromptCloseReason) => void;
  /** This window answered the first prompt. */
  answered: (promptId: string) => void;
  dismiss: () => void;
}

const stores = new WeakMap<UiStore, StoreApi<PinUiState>>();

export function createPinStore(): StoreApi<PinUiState> {
  return createStore<PinUiState>()((set) => ({
    prompts: [],
    closed: null,
    request: (prompt) => {
      set((s) => ({
        prompts: [...s.prompts.filter((p) => p.prompt_id !== prompt.prompt_id), prompt],
        closed: null,
      }));
    },
    close: (promptId, reason) => {
      set((s) => {
        const shown = s.prompts[0]?.prompt_id === promptId;
        return {
          prompts: s.prompts.filter((p) => p.prompt_id !== promptId),
          closed: shown && reason !== "answered" ? reason : s.closed,
        };
      });
    },
    answered: (promptId) => {
      set((s) => ({ prompts: s.prompts.filter((p) => p.prompt_id !== promptId) }));
    },
    dismiss: () => {
      set({ closed: null });
    },
  }));
}

/** The PIN store of a UI store. */
export function pinStoreFor(ui: UiStore): StoreApi<PinUiState> {
  let store = stores.get(ui);
  if (!store) {
    store = createPinStore();
    stores.set(ui, store);
  }
  return store;
}

/** Shows PIN prompts and closes them; refreshes the card list. */
export const handlePinEvent: FeatureEventHandler = (event, { queryClient, store }) => {
  switch (event.type) {
    case "PinRequested":
      pinStoreFor(store).getState().request(event.prompt);
      store.getState().openWindow(PIN_DIALOG);
      break;
    case "PinPromptClosed":
      pinStoreFor(store).getState().close(event.prompt_id, event.reason);
      if (
        pinStoreFor(store).getState().prompts.length === 0 &&
        pinStoreFor(store).getState().closed === null
      ) {
        store.getState().closeWindow(PIN_DIALOG);
      }
      break;
    case "SmartcardsChanged":
      void queryClient.invalidateQueries({ queryKey: smartcardKeys.tokens });
      break;
  }
};

/** The form for one prompt (a new prompt mounts a fresh, empty form). */
function PinForm({ prompt, close }: { prompt: PinPrompt; close: () => void }) {
  const t = useT();
  const format = useDateFormat();
  const client = useClient();
  const queryClient = useQueryClient();
  const ui = useUiStoreApi();
  const id = useId();
  const [pin, setPin] = useState("");
  const { busy, problem, run } = useBusyAction();
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => inputRef.current?.focus(), []);

  const answer = async (value: string | null) => {
    const pins = pinStoreFor(ui);
    await run(async () => {
      await client.request(
        { type: "AnswerPinPrompt", prompt_id: prompt.prompt_id, pin: value },
        "Done",
      );
      setPin("");
      pins.getState().answered(prompt.prompt_id);
      void queryClient.invalidateQueries({ queryKey: smartcardKeys.tokens });
      // A wrong PIN comes back as a new prompt; nothing waiting → done.
      if (pins.getState().prompts.length === 0) close();
    });
  };

  const submit = (event: SubmitEvent) => {
    event.preventDefault();
    if (pinIsPlausible(pin)) void answer(pin);
  };
  const lastTry = prompt.attempts_left !== null && prompt.attempts_left <= 1;
  return (
    <form data-part="pin-prompt" data-purpose={prompt.purpose} onSubmit={submit}>
      <p data-part="pin-prompt.purpose">{t(PIN_PURPOSE_MESSAGE_IDS[prompt.purpose])}</p>
      <p data-part="pin-prompt.token">
        {t("security-pin-token", { label: prompt.token_label, serial: prompt.token_serial })}
      </p>
      {prompt.key_label && (
        <p data-part="pin-prompt.key">{t("security-pin-key", { key: prompt.key_label })}</p>
      )}
      {prompt.retry && (
        <p data-part="form.hint" data-tone="danger" role="alert">
          {t("smartcard-error-pin-incorrect")}
        </p>
      )}
      {prompt.attempts_left !== null && (
        <p data-part="form.hint" data-tone={lastTry ? "danger" : "warning"} data-field="attempts">
          {t(lastTry ? "security-pin-last-attempt" : "security-pin-attempts", {
            count: prompt.attempts_left,
          })}
        </p>
      )}
      <div data-part="form.field" data-field="pin">
        <label data-part="form.label" htmlFor={id}>
          {t("security-pin-label")}
        </label>
        <input
          ref={inputRef}
          id={id}
          data-part="form.input"
          data-field="pin"
          type="password"
          autoComplete="off"
          spellCheck={false}
          value={pin}
          aria-describedby={`${id}-hint`}
          onChange={(e) => {
            setPin(e.target.value);
          }}
        />
        <p data-part="form.hint" id={`${id}-hint`}>
          {t("security-pin-hint", {
            until: format.dateTime(prompt.expires_at, { timeStyle: "short" }),
          })}
        </p>
      </div>
      {problem && (
        <p data-part="form.error" role="alert">
          {t(problem.id, problem.args)}
        </p>
      )}
      <div data-part="wizard.buttons">
        <button
          type="submit"
          data-part="form.button"
          data-variant="primary"
          data-action="answer"
          disabled={busy || !pinIsPlausible(pin)}
        >
          {t("common-ok")}
        </button>
        <button
          type="button"
          data-part="form.button"
          data-action="cancel"
          disabled={busy}
          onClick={() => void answer(null)}
        >
          {t("common-cancel")}
        </button>
      </div>
    </form>
  );
}

/** The PIN dialog's content. */
export function PinPromptView({ close }: { close: () => void }) {
  const t = useT();
  const client = useClient();
  const ui = useUiStoreApi();
  const pins = pinStoreFor(ui);
  const prompt = useStore(pins, (s) => s.prompts[0] ?? null);
  const closed = useStore(pins, (s) => s.closed);

  // Closing the dialog without an answer cancels what is still waiting.
  useEffect(
    () => () => {
      setTimeout(() => {
        if (ui.getState().windows.includes(PIN_DIALOG)) return; // re-mounted
        for (const waiting of pins.getState().prompts) {
          void client.call({ type: "AnswerPinPrompt", prompt_id: waiting.prompt_id, pin: null });
          pins.getState().answered(waiting.prompt_id);
        }
        pins.getState().dismiss();
      }, 0);
    },
    [client, pins, ui],
  );

  if (prompt) return <PinForm key={prompt.prompt_id} prompt={prompt} close={close} />;
  return (
    <div data-part="pin-prompt" data-state="closed">
      <p data-part="pin-prompt.text" role="status">
        {t(closed ? PIN_PROMPT_CLOSE_REASON_MESSAGE_IDS[closed] : "security-pin-nothing")}
      </p>
      <div data-part="wizard.buttons">
        <button
          type="button"
          data-part="form.button"
          data-action="close"
          onClick={() => {
            pins.getState().dismiss();
            close();
          }}
        >
          {t("common-close")}
        </button>
      </div>
    </div>
  );
}

export function PinPromptDialog({ close }: DialogProps) {
  return <PinPromptView close={close} />;
}
