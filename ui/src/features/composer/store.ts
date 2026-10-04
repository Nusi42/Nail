/**
 * The rich part of the message being written: mode, HTML / Markdown
 * source, attachments and composer UI state. The envelope (identity,
 * recipients, subject, plain text, context, draft id) stays in the UI
 * store: `composers[<window id>]` for a composer window `composer:<key>`
 * (M5c, several composers), `composer` for the plain `composer` window.
 *
 * - **Instance composers** (`composer:<key>`, ADR 0116 / WP172): one rich
 *   store per window, `richStoreFor(uiStore, windowId)`; created when the
 *   flow opens the window (`./sessions.ts`), dropped when it closes.
 * - **The plain composer** (`composer`, the M1–M5b single composer, still
 *   used by a layout's `window.open "composer"` and by tests): one store per
 *   UI store, `richStoreFor(uiStore)`. A new session resets it unless a
 *   flow primed it first (`primeRich`).
 *
 * Components inside a composer find their window through
 * `ComposerWindowContext` (`useComposerWindow`).
 */
import { createContext, useContext } from "react";
import { useStore } from "zustand";
import { createStore, type StoreApi } from "zustand/vanilla";
import type { ComposerMode, DraftAttachment, SignatureChoice } from "../../api/generated";
import { useUiStoreApi, type UiStore } from "../../state/uiStore";
import { parseWindowId } from "../../state/windows";

/** The window kind of composers (`composer`, `composer:<key>`). */
export const COMPOSER_KIND = "composer";

/** Idle time after the last edit before the draft is saved. */
export const AUTOSAVE_MS = 1500;

/**
 * The global a test sets to change the autosave delay (ms, a non-negative
 * number): unit tests and Playwright scenarios that check "unsaved
 * changes" flows lengthen it, so the autosave cannot race them under load.
 */
export const AUTOSAVE_OVERRIDE_GLOBAL = "__nailAutosaveMs";

/** The autosave delay: `AUTOSAVE_MS`, or the test override when one is set. */
export function autosaveDelay(): number {
  const override: unknown = (globalThis as Record<string, unknown>)[AUTOSAVE_OVERRIDE_GLOBAL];
  return typeof override === "number" && Number.isFinite(override) && override >= 0
    ? override
    : AUTOSAVE_MS;
}

/** Sets (or with `null` removes) the autosave delay override (tests). */
export function setAutosaveDelay(ms: number | null): void {
  (globalThis as Record<string, unknown>)[AUTOSAVE_OVERRIDE_GLOBAL] = ms ?? undefined;
}

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
const instanceStores = new WeakMap<UiStore, Map<string, RichStore>>();
/** Inline image previews of closed composers (an undone send reopens them). */
const previewCaches = new WeakMap<UiStore, Record<string, string>>();

/** Whether `window` is a composer instance id (`composer:<key>`). */
export function isComposerInstance(window: string | null | undefined): window is string {
  if (!window) return false;
  const parts = parseWindowId(window);
  return parts !== null && parts.kind === COMPOSER_KIND && parts.instance !== null;
}

/** Inline previews remembered from composers closed in this UI store. */
export function cachedPreviews(ui: UiStore): Record<string, string> {
  return previewCaches.get(ui) ?? {};
}

function rememberPreviews(ui: UiStore, previews: Record<string, string>): void {
  if (Object.keys(previews).length === 0) return;
  previewCaches.set(ui, { ...cachedPreviews(ui), ...previews });
}

/** The rich store of an open instance composer, `undefined` when there is none. */
export function instanceRichStore(ui: UiStore, window: string): RichStore | undefined {
  return instanceStores.get(ui)?.get(window);
}

/** Forgets an instance composer's rich store (its window closed). */
export function dropRichStore(ui: UiStore, window: string): void {
  const map = instanceStores.get(ui);
  const store = map?.get(window);
  if (!map || !store) return;
  rememberPreviews(ui, store.getState().inlinePreviews);
  map.delete(window);
}

/**
 * The feature store of a composer: of the window `window` when it is an
 * instance id (created on first use, starting with the remembered inline
 * previews), else of the plain composer of the UI store.
 */
export function richStoreFor(ui: UiStore, window: string | null = null): RichStore {
  if (isComposerInstance(window)) {
    let map = instanceStores.get(ui);
    if (!map) {
      map = new Map();
      instanceStores.set(ui, map);
    }
    let store = map.get(window);
    if (!store) {
      store = createStore<RichState>()(() => ({
        ...emptyRich(),
        inlinePreviews: cachedPreviews(ui),
      }));
      map.set(window, store);
    }
    return store;
  }
  return plainRichStore(ui);
}

function plainRichStore(ui: UiStore): RichStore {
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

/**
 * The plain composer's rich state (for moving its message into an instance
 * window), with the "primed" flag cleared.
 */
export function plainRichSnapshot(ui: UiStore): RichState {
  return { ...plainRichStore(ui).getState(), primed: false };
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

/**
 * The composer window a component is rendered for: `composer:<key>` for an
 * instance composer, `composer` for the plain one (the default).
 */
export const ComposerWindowContext = createContext<string>(COMPOSER_KIND);

/** The composer window of the calling component (see `ComposerWindowContext`). */
export function useComposerWindow(): string {
  return useContext(ComposerWindowContext);
}

/** The feature store of the surrounding composer. */
export function useRichStoreApi(): RichStore {
  return richStoreFor(useUiStoreApi(), useComposerWindow());
}

/** Selects from the feature store of the surrounding composer. */
export function useRich<T>(selector: (state: RichState) => T): T {
  return useStore(useRichStoreApi(), selector);
}
