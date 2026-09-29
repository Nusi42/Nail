/**
 * Provides the action dispatcher and installs the layout's key bindings.
 */
import { useQueryClient } from "@tanstack/react-query";
import { createContext, useContext, useEffect, useMemo, type ReactNode } from "react";
import type { JsonValue, ResolvedLayout } from "../api/generated";
import { useClient } from "../state/queries";
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
  const dispatcher = useMemo(
    () => createDispatcher({ client, store, queryClient, layout }, handlers),
    [client, store, queryClient, layout, handlers],
  );

  // Track the keyboard context of whatever gets focus.
  useEffect(() => {
    const onFocus = (event: FocusEvent) => {
      const context = contextOf(event.target instanceof Element ? event.target : null);
      if (context) store.getState().setKeyContext(context);
    };
    document.addEventListener("focusin", onFocus);
    return () => document.removeEventListener("focusin", onFocus);
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
      if (event.defaultPrevented || event.isComposing || isEditable(event.target)) return;
      const combo = comboFromEvent(event);
      if (!combo) return;
      const focused =
        event.target instanceof Element && event.target !== document.body ? event.target : null;
      const context = contextOf(focused) ?? store.getState().keyContext;
      const result = matcher.feed(combo, [context, "global"]);
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

/**
 * Whether an action is enabled, re-evaluated when the UI state it depends
 * on changes.
 */
export function useActionEnabled(id: string, args: readonly JsonValue[] = []): boolean {
  const dispatcher = useActions();
  // Subscribe to the state the built-in `enabled` checks read.
  useUi((s) => s.messageOrder.length > 0);
  useUi((s) => s.selectedMessageId);
  return dispatcher.isEnabled(id, args);
}
