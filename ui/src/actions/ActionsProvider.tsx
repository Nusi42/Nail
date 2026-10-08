/**
 * Provides the action dispatcher and installs the layout's key bindings.
 */
import { skipToken, useQuery, useQueryClient } from "@tanstack/react-query";
import { createContext, useContext, useEffect, useMemo, type ReactNode } from "react";
import type { JsonValue, ResolvedLayout } from "../api/generated";
import { useLocale, useT } from "../i18n/I18nProvider";
import { useDialogRegistry } from "../layout/dialogRegistry";
import { queryKeys, useClient } from "../state/queries";
import { useUi, useUiStoreApi } from "../state/uiStore";
import { KeyMatcher, comboFromEvent } from "./keys";
import {
  builtinActions,
  createDispatcher,
  type ActionDispatcher,
  type ActionHandler,
} from "./registry";

const ActionsContext = createContext<ActionDispatcher | null>(null);

/** How long a key sequence (`g r`) waits for its next key. */
export const SEQUENCE_TIMEOUT_MS = 1000;

/** The keyboard context of an element: its closest `data-key-context`. */
export function contextOf(element: Element | null): string | null {
  return element?.closest("[data-key-context]")?.getAttribute("data-key-context") ?? null;
}

/**
 * All keyboard contexts around an element, innermost first (e.g.
 * `["composer", "window"]`), so an inner context falls back to outer ones.
 */
export function contextsOf(element: Element | null): string[] {
  const out: string[] = [];
  let current = element?.closest("[data-key-context]") ?? null;
  while (current) {
    const context = current.getAttribute("data-key-context");
    if (context && !out.includes(context)) out.push(context);
    current = current.parentElement?.closest("[data-key-context]") ?? null;
  }
  return out;
}

/**
 * Whether a key typed into a text field may trigger a binding: only chords
 * with Ctrl / Alt / Meta and Escape, so typing text never runs actions.
 */
export function isFieldCombo(combo: string): boolean {
  return /^(ctrl|alt|meta)\+/.test(combo) || combo === "escape";
}

/** Controls whose own Enter / Space activation must win over key bindings. */
const ACTIVATABLE =
  'button, a[href], summary, [role="button"], [role="link"], [role="menuitem"], [role="tab"], [role="checkbox"], [role="radio"], [role="switch"]';

/**
 * Whether a key is a control's own activation (Enter or Space on a button,
 * link, menu item …): bindings never take it (P2: the list's `enter`
 * binding must not swallow a focused button's Enter).
 */
export function isActivation(combo: string, target: EventTarget | null): boolean {
  if (combo !== "enter" && combo !== " ") return false;
  return target instanceof Element && target.closest(ACTIVATABLE) !== null;
}

/**
 * The keys a browser reloads the page with (ADR 0183). Nail is never
 * reloaded by them: wherever the focus is they run their binding (F5 and
 * Ctrl+R: Check for new mail by default), or nothing.
 */
export const RELOAD_KEYS: ReadonlySet<string> = new Set([
  "f5",
  "shift+f5",
  "ctrl+f5",
  "ctrl+r",
  "ctrl+shift+r",
]);

function isEditable(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName);
}

export interface ActionsProviderProps {
  layout: ResolvedLayout;
  handlers?: Record<string, ActionHandler>;
  children: ReactNode;
}

export function ActionsProvider({
  layout,
  handlers = builtinActions,
  children,
}: ActionsProviderProps) {
  const client = useClient();
  const store = useUiStoreApi();
  const queryClient = useQueryClient();
  const t = useT();
  const locale = useLocale();
  const dialogRegistry = useDialogRegistry();
  const dialogs = useMemo(() => new Set(Object.keys(dialogRegistry)), [dialogRegistry]);
  const dispatcher = useMemo(
    () => createDispatcher({ client, store, queryClient, layout, t, locale, dialogs }, handlers),
    [client, store, queryClient, layout, handlers, t, locale, dialogs],
  );

  // Track the keyboard context of whatever gets focus.
  useEffect(() => {
    const onFocus = (event: FocusEvent) => {
      const context = contextOf(event.target instanceof Element ? event.target : null);
      if (context) store.getState().setKeyContext(context);
    };
    document.addEventListener("focusin", onFocus);
    return () => {
      document.removeEventListener("focusin", onFocus);
    };
  }, [store]);

  // Key bindings.
  useEffect(() => {
    const matcher = new KeyMatcher(layout.keys);
    let timer: ReturnType<typeof setTimeout> | null = null;
    const clearTimer = () => {
      if (timer !== null) clearTimeout(timer);
      timer = null;
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.isComposing) return;
      const combo = comboFromEvent(event);
      if (!combo || isActivation(combo, event.target)) return;
      const focused =
        event.target instanceof Element && event.target !== document.body ? event.target : null;
      const around = contextsOf(focused);
      let contexts: string[];
      if (RELOAD_KEYS.has(combo)) {
        event.preventDefault();
        if (matcher.pending) return;
        contexts = [...around, "global"];
      } else if (isEditable(event.target)) {
        // In text fields only the field's own contexts apply, and only chords.
        if (!isFieldCombo(combo) || around.length === 0 || matcher.pending) return;
        contexts = around;
      } else if (focused?.closest("[data-key-modal]")) {
        // Inside a modal window, main-window (global) bindings do not apply.
        contexts = around;
      } else if (
        store.getState().windows.length > 0 &&
        document.querySelector('[data-part="window"][data-owner="main"]') !== null
      ) {
        // A modal window (a confirmation, M5c) blocks the main window: only
        // window bindings apply. Non-modal windows, and confirmations of
        // other windows, leave the main window's bindings working.
        contexts = ["window"];
      } else {
        const own = around.length > 0 ? around : [store.getState().keyContext];
        contexts = [...own, "global"];
      }
      const result = matcher.feed(combo, contexts);
      clearTimer();
      if (result.kind === "none") return;
      event.preventDefault();
      if (result.kind === "match") {
        void dispatcher.dispatch(result.match.action, result.match.args);
      } else {
        timer = setTimeout(() => {
          timer = null;
          const match = matcher.flush();
          if (match) void dispatcher.dispatch(match.action, match.args);
        }, SEQUENCE_TIMEOUT_MS);
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => {
      clearTimer();
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [layout.keys, dispatcher, store]);

  return <ActionsContext.Provider value={dispatcher}>{children}</ActionsContext.Provider>;
}

/** The action dispatcher. */
export function useActions(): ActionDispatcher {
  const dispatcher = useContext(ActionsContext);
  if (!dispatcher) {
    throw new Error("useActions() must be used inside <ActionsProvider>");
  }
  return dispatcher;
}

/** Re-renders when the selected message's cached body changes (no fetch of its own). */
function useSelectedMessageData(): unknown {
  const selected = useUi((s) => s.selectedMessageId);
  return useQuery({ queryKey: queryKeys.message(selected), queryFn: skipToken }).data;
}

/**
 * Whether an action is enabled, re-evaluated when the UI state it depends
 * on changes.
 */
export function useActionEnabled(id: string, args: readonly JsonValue[] = []): boolean {
  const dispatcher = useActions();
  // Subscribe to the state the built-in `enabled` checks read.
  useUi((s) => s.messageOrder.length > 0);
  useUi((s) => s.selectedMessageId);
  useUi((s) => s.composer !== null);
  useUi((s) => s.windows.length);
  useUi((s) => s.lastSend?.state);
  useUi((s) => s.activeFolderId);
  // Checks that read the loaded message (Autocrypt setup, junk verdict).
  useSelectedMessageData();
  return dispatcher.isEnabled(id, args);
}
