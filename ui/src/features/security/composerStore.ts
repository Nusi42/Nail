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
import { COMPOSER_KIND } from "../composer/store";

/** One composer's latest recipient report and its identity's preference. */
export interface ComposerReport {
  report: RecipientKeysReport | null;
  preference: CryptoPreference;
}

export interface ComposerCryptoState {
  /** Explicit choice, `null` = follow the suggestion. */
  choice: SendCrypto | null;
  /** The focused composer's latest recipient report (for the send guard). */
  report: RecipientKeysReport | null;
  /** The sending identity's technology preference. */
  preference: CryptoPreference;
  /**
   * Every open composer's latest report by window (R8): a composer in the
   * background keeps its report current as its recipients change, so the
   * report is right the moment it gets the focus (`activate`).
   */
  reports: Readonly<Record<string, ComposerReport>>;
  /** The focused composer's window (the plain `composer` without several composers). */
  active: string;
  setChoice: (choice: SendCrypto | null) => void;
  /** A new report of the focused composer. */
  setReport: (report: RecipientKeysReport | null, preference: CryptoPreference) => void;
  /** A new report of the composer of `window` (the focused one's becomes `report`). */
  setReportOf: (
    window: string,
    report: RecipientKeysReport | null,
    preference: CryptoPreference,
  ) => void;
  /** The composer of `window` got the focus: its kept report becomes `report`. */
  activate: (window: string) => void;
  /** The composer of `window` closed. */
  forget: (window: string) => void;
  reset: () => void;
}

export const composerCryptoStore = createStore<ComposerCryptoState>()((set, get) => ({
  choice: null,
  report: null,
  preference: "auto",
  reports: {},
  active: COMPOSER_KIND,
  setChoice: (choice) => {
    set({ choice });
  },
  setReport: (report, preference) => {
    get().setReportOf(get().active, report, preference);
  },
  setReportOf: (window, report, preference) => {
    const reports = { ...get().reports, [window]: { report, preference } };
    set(window === get().active ? { reports, report, preference } : { reports });
  },
  activate: (window) => {
    const kept = Object.hasOwn(get().reports, window) ? get().reports[window] : undefined;
    set({ active: window, report: kept?.report ?? null, preference: kept?.preference ?? "auto" });
  },
  forget: (window) => {
    if (!Object.hasOwn(get().reports, window)) return;
    set({
      reports: Object.fromEntries(Object.entries(get().reports).filter(([w]) => w !== window)),
    });
  },
  reset: () => {
    const { active } = get();
    get().forget(active);
    set({ choice: null, report: null, preference: "auto" });
  },
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
