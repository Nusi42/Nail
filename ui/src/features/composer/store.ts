/**
 * The rich part of the message being written: mode, HTML / Markdown
 * source, attachments and composer UI state. The envelope (identity,
 * recipients, subject, plain text, context, draft id) stays in the UI
 * store's `composer`, so the plain-text flows keep working unchanged.
 *
 * There is one feature store per UI store (tests create many), found with
 * `richStoreFor(uiStore)`. A new composer session resets it unless a flow
 * primed it first (`primeRich`), e.g. with the draft `NewDraft` returned.
 */
import { useStore } from "zustand";
import { createStore, type StoreApi } from "zustand/vanilla";
import type { ComposerMode, DraftAttachment, SignatureChoice } from "../../api/generated";
import { useUiStoreApi, type UiStore } from "../../state/uiStore";

/** Idle time after the last edit before the draft is saved. */
export const AUTOSAVE_MS = 1500;

export type AutosaveState = "idle" | "saving" | "saved" | "failed";

export interface PendingSwitch {
  /** The mode the user asked for. */
  mode: ComposerMode;
  /** The converted body, applied when the user confirms. */
  html: string | null;
  markdown: string | null;
  text: string;
}

export interface RichState {
  mode: ComposerMode;
  /** HTML source (html mode). */
  html: string;
  /** Markdown source (markdown mode). */
  markdown: string;
  attachments: DraftAttachment[];
  /** Markdown mode: also send the source as `text/markdown`. */
  markdownPart: boolean;
  signature: SignatureChoice;
  /** Markdown mode: side-by-side preview shown. */
  preview: boolean;
  /** HTML mode: raw source view instead of the rich editor. */
  htmlSource: boolean;
  /** A lossy mode switch waiting for confirmation. */
  pendingSwitch: PendingSwitch | null;
  switching: boolean;
  autosave: AutosaveState;
  /** Draft key (see `draftKey`) of what the core has; `null` = not known yet. */
  savedKey: string | null;
  /** The attachment reminder: `confirm` = shown after Send, waiting for "send anyway". */
  reminder: "confirm" | "dismissed" | null;
  uploads: number;
  /** Local previews of inline images: content id → data URL. */
  inlinePreviews: Record<string, string>;
  /** Bumped by `composer.attach` so the widget opens its file picker. */
  attachRequest: number;
  /** Set by flows before opening a composer, so the session keeps this state. */
  primed: boolean;
}

export function emptyRich(): RichState {
  return {
    mode: "plain",
    html: "",
    markdown: "",
    attachments: [],
    markdownPart: false,
    signature: "identity",
    preview: true,
    htmlSource: false,
    pendingSwitch: null,
    switching: false,
    autosave: "idle",
    savedKey: null,
    reminder: null,
    uploads: 0,
    inlinePreviews: {},
    attachRequest: 0,
    primed: false,
  };
}

export type RichStore = StoreApi<RichState>;

const stores = new WeakMap<UiStore, RichStore>();

/** The feature store belonging to a UI store (created on first use). */
export function richStoreFor(ui: UiStore): RichStore {
  const existing = stores.get(ui);
  if (existing) return existing;
  const rich = createStore<RichState>()(() => emptyRich());
  stores.set(ui, rich);
  let open = ui.getState().composer !== null;
  ui.subscribe((state) => {
    const nowOpen = state.composer !== null;
    if (nowOpen === open) return;
    open = nowOpen;
    const current = rich.getState();
    if (!nowOpen) {
      // Keep previews: an undone send reopens the same images.
      rich.setState({ ...emptyRich(), inlinePreviews: current.inlinePreviews });
    } else if (current.primed) {
      rich.setState({ primed: false });
    } else {
      rich.setState({ ...emptyRich(), inlinePreviews: current.inlinePreviews });
    }
  });
  return rich;
}

/** Sets the rich state for the composer about to be opened. */
export function primeRich(ui: UiStore, patch: Partial<RichState>): void {
  const rich = richStoreFor(ui);
  const open = ui.getState().composer !== null;
  rich.setState({
    ...emptyRich(),
    inlinePreviews: rich.getState().inlinePreviews,
    ...patch,
    primed: !open,
  });
}

/** The feature store of the surrounding UI store. */
export function useRichStoreApi(): RichStore {
  return richStoreFor(useUiStoreApi());
}

/** Selects from the feature store of the surrounding UI store. */
export function useRich<T>(selector: (state: RichState) => T): T {
  return useStore(useRichStoreApi(), selector);
}
