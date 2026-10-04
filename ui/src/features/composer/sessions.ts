/**
 * Several composers (M5c WP172, ADR 0116): one window per draft,
 * `composer:<key>` (the draft id, or `w1`, `w2`, … before the first save),
 * each with its envelope in `UiState.composers[<id>]` and its rich state in
 * its own store (`richStoreFor(ui, id)`).
 *
 * **The focused composer.** The single-instance readers outside this
 * feature (sign / encrypt toggles and send guard, plug-in context and
 * panel, MCP UI context, command-palette contexts, toolbar / action
 * `enabled`) read `UiState.composer`. While instance composers are open,
 * that field mirrors the *focused* one — the topmost `composer:<key>` in
 * the window z-order — so they all act on the composer the user works in.
 * A write to the mirror (an older flow) goes back to that composer;
 * `composer = null` closes it. Without instance composers, `composer` is
 * the plain `composer` window's own state (M1–M5b, tests).
 *
 * The sign / encrypt choice lives in one store (`../security`); it belongs
 * to the focused composer: on a focus change the outgoing composer's choice
 * is kept here and the incoming one's restored. Composers in the background
 * save their kept choice (`cryptoFor`).
 *
 * **Closing.** A composer closed by a flow (sent, discarded, closed after
 * the confirmation) goes away. Closed any other way (its title bar's close
 * button, Escape, a view switch) it goes away too when nothing is unsaved;
 * with unsaved changes it stays open and the modal confirmation
 * `composer-close:<key>` asks (save, close without saving, keep editing).
 */
import type { DraftId, SendCrypto } from "../../api/generated";
import type { ComposerState, UiState, UiStore } from "../../state/uiStore";
import {
  isWindowInstance,
  newWindowInstance,
  parseWindowId,
  windowId,
  windowKind,
} from "../../state/windows";
import { composerCryptoStore, loadComposerCrypto } from "../security";
import { draftKey, type CryptoChoice } from "./draft";
import { composerOf } from "./windowOf";
import {
  COMPOSER_KIND,
  dropRichStore,
  instanceRichStore,
  isComposerInstance,
  plainRichSnapshot,
  richStoreFor,
} from "./store";

export { composerOf };

/** The modal "close with unsaved changes?" confirmation (`composer-close:<key>`). */
export const CLOSE_CONFIRM_KIND = "composer-close";
/** The modal "discard this message?" confirmation (`composer-discard:<key>`). */
export const DISCARD_CONFIRM_KIND = "composer-discard";

/** Instance composer ids in `windows`, in z-order (topmost last). */
export function composerInstances(windows: readonly string[]): string[] {
  return windows.filter(isComposerInstance);
}

/** The focused instance composer: the topmost `composer:<key>`, if any. */
export function activeComposer(windows: readonly string[]): string | null {
  for (let i = windows.length - 1; i >= 0; i -= 1) {
    const id = windows[i];
    if (isComposerInstance(id)) return id;
  }
  return null;
}

/**
 * The composer a flow works on: `window` when given, else the focused
 * instance composer, else the plain `composer`.
 */
export function composerTarget(state: Pick<UiState, "windows">, window?: string | null): string {
  return window ?? activeComposer(state.windows) ?? COMPOSER_KIND;
}

/** Sets (`null` removes) the envelope of a composer window. */
export function setComposerFor(ui: UiStore, window: string, composer: ComposerState | null): void {
  if (isComposerInstance(window)) ui.getState().setComposerOf(window, composer);
  else ui.getState().setComposer(composer);
}

/** Patches the envelope of a composer window. */
export function patchComposer(ui: UiStore, window: string, patch: Partial<ComposerState>): void {
  if (isComposerInstance(window)) ui.getState().updateComposerOf(window, patch);
  else ui.getState().updateComposer(patch);
}

/** The open instance composer of a draft, if any. */
export function findComposerByDraft(
  state: Pick<UiState, "windows" | "composers">,
  draftId: DraftId,
): string | null {
  for (const id of composerInstances(state.windows)) {
    if (state.composers[id]?.draftId === draftId) return id;
  }
  return null;
}

/**
 * The window id for a new composer: `composer:<draft id>` when the draft has
 * an id that is a valid instance key and no window uses it, else the next
 * free `composer:w<n>`.
 */
export function newComposerWindow(
  state: Pick<UiState, "windows" | "composers">,
  draftId: DraftId | null,
): string {
  const taken = [...state.windows, ...Object.keys(state.composers)];
  if (draftId !== null && isWindowInstance(draftId)) {
    const id = windowId(COMPOSER_KIND, draftId);
    if (!taken.includes(id)) return id;
  }
  return newWindowInstance(COMPOSER_KIND, taken);
}

/** The confirmation window `kind` for a composer (same instance key). */
export function confirmWindowFor(kind: string, composer: string): string {
  return windowId(kind, parseWindowId(composer)?.instance ?? null);
}

/** The composer a confirmation window (`composer-close:<key>`, …) belongs to. */
export function composerOfConfirm(confirm: string): string {
  return windowId(COMPOSER_KIND, parseWindowId(confirm)?.instance ?? null);
}

interface Session {
  /** The instance composer `UiState.composer` mirrors. */
  active: string | null;
  /** Sign / encrypt choices of composers in the background. */
  crypto: Map<string, SendCrypto | null>;
  /** Windows a flow is closing (no confirmation). */
  closing: Set<string>;
  applying: boolean;
}

const sessions = new WeakMap<UiStore, Session>();

/**
 * The composer sessions of a UI store, installing the sync (mirror,
 * closing) on first use. Flows call it before they open a composer.
 */
export function composerSessions(ui: UiStore): Session {
  const existing = sessions.get(ui);
  if (existing) return existing;
  const session: Session = { active: null, crypto: new Map(), closing: new Set(), applying: false };
  sessions.set(ui, session);
  ui.subscribe((next, prev) => {
    if (session.applying) return;
    const patch = reconcile(ui, session, prev, next);
    if (patch === null) return;
    session.applying = true;
    try {
      ui.setState(patch);
    } finally {
      session.applying = false;
    }
  });
  // A store that already has instance windows (restored state, tests).
  const patch = reconcile(ui, session, { ...ui.getState(), windows: [] }, ui.getState());
  if (patch !== null) ui.setState(patch);
  return session;
}

/** Marks a composer window as closed by a flow: no "unsaved" confirmation. */
export function markClosing(ui: UiStore, window: string): void {
  composerSessions(ui).closing.add(window);
}

/** The sign / encrypt choice to save with a composer (see `CryptoChoice`). */
export function cryptoFor(ui: UiStore, window: string): CryptoChoice {
  const session = sessions.get(ui);
  if (!isComposerInstance(window) || !session || session.active === window) return undefined;
  return session.crypto.get(window) ?? null;
}

/**
 * Sets the sign / encrypt choice of a composer about to open (a saved
 * draft's `crypto`); it applies when the composer is focused.
 */
export function setCryptoFor(ui: UiStore, window: string, choice: SendCrypto | null): void {
  const session = composerSessions(ui);
  if (session.active === window) loadComposerCrypto(choice);
  else session.crypto.set(window, choice);
}

/**
 * Whether a composer has changes the core does not have: its draft key
 * differs from the last saved (or opened) one. A composer that never
 * rendered has no baseline and nothing unsaved.
 */
export function isUnsaved(ui: UiStore, window: string): boolean {
  const composer = composerOf(ui.getState(), window);
  const rich = isComposerInstance(window)
    ? instanceRichStore(ui, window)?.getState()
    : richStoreFor(ui).getState();
  if (!composer || !rich) return false;
  if (rich.savedKey === null) return false;
  return draftKey(composer, rich, cryptoFor(ui, window)) !== rich.savedKey;
}

function withoutKey<T>(record: Readonly<Record<string, T>>, key: string): Record<string, T> {
  return Object.fromEntries(Object.entries(record).filter(([k]) => k !== key));
}

/**
 * One store change → the patch that keeps the composers consistent (and
 * the side effects on the per-window stores), `null` when nothing is due.
 */
function reconcile(
  ui: UiStore,
  session: Session,
  prev: UiState,
  next: UiState,
): Partial<UiState> | null {
  let windows = next.windows;
  let composers = next.composers;
  let composer = next.composer;
  const drop = (id: string) => {
    composers = withoutKey(composers, id);
    dropRichStore(ui, id);
    session.crypto.delete(id);
    session.closing.delete(id);
    // Its confirmations have nothing left to ask about.
    const confirms = [CLOSE_CONFIRM_KIND, DISCARD_CONFIRM_KIND].map((k) => confirmWindowFor(k, id));
    windows = windows.filter((w) => !confirms.includes(w));
  };

  // 1. Instance composers whose window closed.
  if (windows !== prev.windows) {
    for (const id of composerInstances(prev.windows)) {
      if (windows.includes(id)) continue;
      if (session.closing.has(id) || !isUnsaved(ui, id)) {
        drop(id);
      } else {
        // Unsaved: it stays open, and the confirmation asks.
        windows = [...windows, id, confirmWindowFor(CLOSE_CONFIRM_KIND, id)];
      }
    }
  }

  // 2. The plain `composer` window next to instance composers: an open
  // message moves into an instance window; an empty one gives way.
  if (windows.includes(COMPOSER_KIND) && composerInstances(windows).length > 0) {
    const mirrored = session.active !== null && composer === composers[session.active];
    if (composer !== null && !mirrored) {
      const id = newComposerWindow({ windows, composers }, composer.draftId);
      composers = { ...composers, [id]: composer };
      richStoreFor(ui, id).setState(plainRichSnapshot(ui));
      windows = windows.map((w) => (w === COMPOSER_KIND ? id : w));
    } else {
      windows = windows.filter((w) => w !== COMPOSER_KIND);
    }
  }

  // 3. The mirror of the focused composer.
  const active = activeComposer(windows);
  if (active !== session.active) {
    const before = session.active;
    if (before !== null && windows.includes(before)) {
      session.crypto.set(before, composerCryptoStore.getState().choice);
    }
    if (active !== null) loadComposerCrypto(session.crypto.get(active) ?? null);
    session.active = active;
    if (active !== null) composer = composers[active] ?? null;
    else if (before !== null) composer = windows.includes(COMPOSER_KIND) ? composer : null;
  } else if (active !== null) {
    const own = composers[active] ?? null;
    if (next.composer !== prev.composer && next.composer !== own) {
      // An older flow wrote the mirror: it means the focused composer.
      if (next.composer === null) {
        windows = windows.filter((w) => w !== active);
        drop(active);
        const now = activeComposer(windows);
        session.active = now;
        if (now !== null) loadComposerCrypto(session.crypto.get(now) ?? null);
        composer = now !== null ? (composers[now] ?? null) : null;
      } else {
        composers = { ...composers, [active]: next.composer };
      }
    } else if (composer !== own) {
      composer = own;
    }
  }

  const patch: Partial<UiState> = {};
  if (windows !== next.windows) patch.windows = windows;
  if (composers !== next.composers) patch.composers = composers;
  if (composer !== next.composer) patch.composer = composer;
  return Object.keys(patch).length > 0 ? patch : null;
}

/** Window kinds this feature owns (for `windowKind` checks in tests). */
export function isComposerWindow(window: string): boolean {
  const kind = windowKind(window);
  return kind === COMPOSER_KIND || kind === CLOSE_CONFIRM_KIND || kind === DISCARD_CONFIRM_KIND;
}
