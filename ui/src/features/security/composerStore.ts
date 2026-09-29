/**
 * The composer's sign / encrypt choice (Zustand, outside `uiStore`).
 *
 * `choice === null` means "not touched": the composer follows the core's
 * suggestion (`RecipientKeysReport.suggested`: identity defaults,
 * Autocrypt). Once the user flips a toggle, the explicit choice is kept
 * and goes into `Draft.crypto` (autosave) and `SendDraft.crypto`.
 *
 * Integration (WP35, see ADR 0033): the composer calls
 * `loadComposerCrypto(draft.crypto)` when it opens a draft, puts
 * `composerCrypto()` into `Draft.crypto` / `SendDraft.crypto`, and asks
 * `composerSendBlock()` before sending.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type { CryptoPreference, RecipientKeysReport, SendCrypto } from "../../api/generated";
import { sendBlock, type SendBlock } from "./model";

export interface ComposerCryptoState {
  /** Explicit choice, `null` = follow the suggestion. */
  choice: SendCrypto | null;
  /** The latest recipient report (for the send guard). */
  report: RecipientKeysReport | null;
  /** The sending identity's technology preference. */
  preference: CryptoPreference;
  setChoice(choice: SendCrypto | null): void;
  setReport(report: RecipientKeysReport | null, preference: CryptoPreference): void;
  reset(): void;
}

export const composerCryptoStore = createStore<ComposerCryptoState>()((set) => ({
  choice: null,
  report: null,
  preference: "auto",
  setChoice: (choice) => set({ choice }),
  setReport: (report, preference) => set({ report, preference }),
  reset: () => set({ choice: null, report: null, preference: "auto" }),
}));

export function useComposerCrypto<T>(selector: (state: ComposerCryptoState) => T): T {
  return useStore(composerCryptoStore, selector);
}

/** The effective choice: explicit, else the core's suggestion, else `null` (defaults). */
export function effectiveChoice(state: Pick<ComposerCryptoState, "choice" | "report">) {
  return state.choice ?? state.report?.suggested ?? null;
}

/**
 * What the composer puts into `Draft.crypto` / `SendDraft.crypto`: the
 * explicit choice, or `undefined` to leave it to the identity defaults.
 */
export function composerCrypto(): SendCrypto | undefined {
  return composerCryptoStore.getState().choice ?? undefined;
}

/**
 * What the composer sends with `SendDraft.crypto`: what the toggles show
 * (the explicit choice, else the core's suggestion), so the message is
 * protected exactly as displayed; `undefined` before the first report.
 */
export function composerSendCrypto(): SendCrypto | undefined {
  return effectiveChoice(composerCryptoStore.getState()) ?? undefined;
}

/** Restores the choice saved with a draft (`Draft.crypto`). */
export function loadComposerCrypto(crypto: SendCrypto | null | undefined): void {
  composerCryptoStore.setState({ choice: crypto ?? null });
}

/** Why sending must wait or is refused with the current choice, `null` = go. */
export function composerSendBlock(): SendBlock | null {
  const s = composerCryptoStore.getState();
  const choice = effectiveChoice(s);
  return choice ? sendBlock(choice, s.preference, s.report) : null;
}
