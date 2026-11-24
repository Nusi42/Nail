/**
 * The window layer's close guard (R6, ADR 0239; generalised from the rules
 * window's, ADR 0222): × , Esc and Cancel all close a window the same way —
 * `closeWindow` — and none of them commits. A window with unsaved changes
 * registers a guard (`useCloseGuard`); when it leaves the open windows while
 * its guard says it is dirty, it is put back in the same store update
 * (before React renders, so it keeps its state) and the shared prompt asks
 * (`UNSAVED_PROMPT`, `../widgets/dialog/UnsavedPrompt.tsx`):
 *
 * - "Save changes to “X”?" — Save (`save`; the window closes when it
 *   succeeds, stays open with its inline error when not), Don't save
 *   (`discard`, then it closes), Cancel (keep editing);
 * - when saving is impossible (`canSave` false: the item is invalid) —
 *   Discard changes, Keep editing.
 *
 * Leaving something inside a window (another settings section, another
 * rule) asks the same way: `askToLeave(ui, window, proceed)`.
 *
 * When several guarded windows with unsaved changes leave at once (another
 * view closes every window), one prompt asks about all of them (R8): it
 * lists their names; Save saves each in turn (the first that fails stays
 * open), Don't save drops every change (`SEVERAL_GUARD`, the question's
 * `several`).
 *
 * A window that closes itself after saving uses `closeWindowNow` (or is no
 * longer dirty by then). Guards are kept per UI store and window id; a
 * guard outside any window is keyed `MAIN_GUARD`.
 */
import { useContext, useEffect, useLayoutEffect, useRef } from "react";
import { useUiStoreApi, type UiStore, UiStoreMap } from "../state/uiStore";
import {
  WindowInstanceContext,
  WINDOW_INSTANCE_SEPARATOR,
  isWindowInstance,
  parseWindowId,
  windowId,
} from "../state/windows";

/** What a window tells its guard. Read when the window closes, so always current. */
export interface CloseGuard {
  /** The name of what is being edited ("Quarterly numbers"), localized. */
  name: () => string;
  /** Whether the window has changes it would lose. */
  isDirty: () => boolean;
  /**
   * Saves the changes: `true` when saved (the window may go), `false` when
   * it failed (it stays, showing why). Left out: nothing to save with.
   */
  save?: () => Promise<boolean> | boolean;
  /** Whether saving is possible now (default: when `save` is given). */
  canSave?: () => boolean;
  /** Drops the changes ("Don't save"); the window then closes or moves on. */
  discard?: () => void;
  /** The Save button's verb ("Save draft"), localized; default "Save". */
  saveLabel?: () => string;
  /** A note under the question ("The draft is saved in Drafts."), localized. */
  detail?: () => string;
}

/** The kind of the shared unsaved-changes prompt (a modal confirmation window). */
export const UNSAVED_PROMPT = "unsaved-changes";

/** The key of a guard outside any secondary window (the main window). */
export const MAIN_GUARD = "main";

/**
 * The prompt key of one question about several windows at once (R8). Never
 * a window of its own; its question names the windows (`several`).
 */
export const SEVERAL_GUARD = "several-windows";

/** What the open prompt asks about. */
export interface UnsavedQuestion {
  /** The guarded window (or `MAIN_GUARD`, or `SEVERAL_GUARD`). */
  window: string;
  /** What happens after Save or Don't save: the window closes, or the move on. */
  proceed: () => void;
  /** `SEVERAL_GUARD`'s question: the guarded windows it asks about, in order. */
  several?: readonly string[];
}

interface GuardState {
  guards: Map<string, CloseGuard>;
  questions: Map<string, UnsavedQuestion>;
  /** Windows closing without asking (`closeWindowNow`, after an answer). */
  forcing: Set<string>;
  installed: boolean;
}

const states = new UiStoreMap<GuardState>();

function stateOf(ui: UiStore): GuardState {
  let state = states.get(ui);
  if (!state) {
    state = { guards: new Map(), questions: new Map(), forcing: new Set(), installed: false };
    states.set(ui, state);
  }
  return state;
}

/**
 * The prompt window for guarded window `window`: `unsaved-changes:<key>`,
 * the key being the window id with `:` as `_` (a kind never has `_`).
 */
export function promptWindowFor(window: string): string {
  const key = window.replace(WINDOW_INSTANCE_SEPARATOR, "_");
  return windowId(UNSAVED_PROMPT, isWindowInstance(key) ? key : MAIN_GUARD);
}

/** The guarded window a prompt window asks about (`null`: not a prompt). */
export function guardedWindowOf(prompt: string): string | null {
  const parts = parseWindowId(prompt);
  if (parts?.kind !== UNSAVED_PROMPT || parts.instance === null) return null;
  return parts.instance.replace("_", WINDOW_INSTANCE_SEPARATOR);
}

/** The guard of `window`, if one is registered. */
export function closeGuardOf(ui: UiStore, window: string): CloseGuard | null {
  return states.get(ui)?.guards.get(window) ?? null;
}

/** The open question about `window`, if any. */
export function unsavedQuestionOf(ui: UiStore, window: string): UnsavedQuestion | null {
  return states.get(ui)?.questions.get(window) ?? null;
}

/** Forgets the question about `window` (the prompt was answered or closed). */
export function forgetUnsavedQuestion(ui: UiStore, question: UnsavedQuestion): void {
  const questions = states.get(ui)?.questions;
  if (questions?.get(question.window) === question) questions.delete(question.window);
}

/** Opens the shared prompt about `window`, `proceed` running after Save or Don't save. */
function ask(ui: UiStore, window: string, proceed: () => void): void {
  stateOf(ui).questions.set(window, { window, proceed });
  ui.getState().openWindow(promptWindowFor(window));
}

/**
 * Moves on inside `window` (another section, another item): at once when
 * it has nothing unsaved, else after the shared prompt's Save or Don't save.
 */
export function askToLeave(ui: UiStore, window: string, proceed: () => void): void {
  const guard = closeGuardOf(ui, window);
  if (guard?.isDirty() !== true) {
    proceed();
    return;
  }
  ask(ui, window, proceed);
}

/**
 * `askToLeave` as a promise: `true` at once when `window` has nothing
 * unsaved, else once the shared prompt's Save or Don't save let it go;
 * `false` when the prompt was cancelled (Cancel, ×, Esc) or a save failed.
 * For moves that happen outside the window (popping it out of the page or
 * back), which must not run before the answer.
 */
export function confirmLeave(ui: UiStore, window: string): Promise<boolean> {
  if (closeGuardOf(ui, window)?.isDirty() !== true) return Promise.resolve(true);
  return new Promise((resolve) => {
    const prompt = promptWindowFor(window);
    let answered = false;
    let shown = false;
    const unsubscribe = ui.subscribe((state) => {
      if (state.windows.includes(prompt)) {
        shown = true;
        return;
      }
      if (!shown) return;
      unsubscribe();
      // Save and Don't save close the prompt, then go on in the same task.
      queueMicrotask(() => {
        if (!answered) resolve(false);
      });
    });
    ask(ui, window, () => {
      answered = true;
      unsubscribe();
      resolve(true);
    });
    shown ||= ui.getState().windows.includes(prompt);
  });
}

/** Closes `window` without asking (after its own Save, or an answer of the prompt). */
export function closeWindowNow(ui: UiStore, window: string): void {
  const state = stateOf(ui);
  state.forcing.add(window);
  try {
    ui.getState().closeWindow(window);
  } finally {
    state.forcing.delete(window);
  }
}

/**
 * Installs the guard on a UI store (once): a guarded window with unsaved
 * changes that leaves the open windows comes back and the prompt asks.
 * Returns the unsubscribe function (`null` when it was installed already).
 */
export function installCloseGuard(ui: UiStore): (() => void) | null {
  const state = stateOf(ui);
  if (state.installed) return null;
  state.installed = true;
  const unsubscribe = ui.subscribe((next, prev) => {
    if (prev.windows === next.windows) return;
    const closed = prev.windows.filter(
      (w) =>
        !next.windows.includes(w) &&
        !state.forcing.has(w) &&
        state.guards.get(w)?.isDirty() === true,
    );
    if (closed.length === 0) return;
    // Back on top, and ask: about the one, or once about all of them (R8).
    ui.setState({ windows: [...next.windows, ...closed] });
    if (closed.length === 1) {
      const [w] = closed;
      if (w !== undefined) {
        ask(ui, w, () => {
          closeWindowNow(ui, w);
        });
      }
      return;
    }
    state.questions.set(SEVERAL_GUARD, {
      window: SEVERAL_GUARD,
      several: closed,
      proceed: () => {
        for (const w of closed) closeWindowNow(ui, w);
      },
    });
    ui.getState().openWindow(promptWindowFor(SEVERAL_GUARD));
  });
  return () => {
    state.installed = false;
    unsubscribe();
  };
}

/** The guard key of the calling component: its window's id, or `MAIN_GUARD`. */
export function useGuardKey(): string {
  return useContext(WindowInstanceContext) ?? MAIN_GUARD;
}

/**
 * Registers `guard` for the window the calling component is rendered in
 * (`MAIN_GUARD` outside a window) while it is mounted. The guard is read
 * when the window closes, so it may close over the latest render's state.
 */
export function useCloseGuard(guard: CloseGuard): void {
  const ui = useUiStoreApi();
  const window = useGuardKey();
  const current = useRef(guard);
  useLayoutEffect(() => {
    current.current = guard;
  });
  useEffect(() => {
    const state = stateOf(ui);
    const stable: CloseGuard = {
      name: () => current.current.name(),
      isDirty: () => current.current.isDirty(),
      save: () => current.current.save?.() ?? false,
      canSave: () => current.current.save !== undefined && (current.current.canSave?.() ?? true),
      discard: () => current.current.discard?.(),
      saveLabel: () => current.current.saveLabel?.() ?? "",
      detail: () => current.current.detail?.() ?? "",
    };
    state.guards.set(window, stable);
    return () => {
      if (state.guards.get(window) === stable) state.guards.delete(window);
    };
  }, [ui, window]);
}
