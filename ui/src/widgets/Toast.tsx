/**
 * The app's one toast (P3, ADR 0164; R1 review): what an action did, with
 * Undo while it can be undone, or why the core refused it. Mail actions
 * (`features/mail-actions/Toast.tsx`) and rule runs from a menu
 * (`features/rules/RunToast.tsx`) both show it; each feature keeps its own
 * state and words, this widget owns the look, the timing and the buttons.
 *
 * - A manual popover (top layer, above every window); where the browser
 *   has no popovers (test DOMs) it shows in place.
 * - It hides after `TOAST_MS[kind]` unless the pointer or the focus is on it.
 * - **One at a time.** Every toast shares one place on screen, so a newer
 *   toast (of any feature) replaces the one shown: the older one is
 *   dismissed (`onDismiss`), never drawn underneath.
 *
 * Parts: `mail-actions.toast` (`data-kind` `done` / `undone` / `error`,
 * `data-source`, `data-action`), `.text`, `.undo`, `.dismiss` (an icon
 * button), `.dismiss.icon`; the look is `MAIL_ACTIONS_SKIN_CSS`.
 */
import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { createStore } from "zustand/vanilla";
import { useT } from "../i18n/I18nProvider";
import { Icon } from "./Icon";

/** What a toast reports. */
export type ToastKind = "done" | "undone" | "error";

/** How long a toast stays (ms) while nobody points at it or focuses it. */
export const TOAST_MS: Readonly<Record<ToastKind, number>> = {
  done: 8000,
  undone: 4000,
  error: 8000,
};

/**
 * Whether the browser has popovers (every engine Nail runs in; test DOMs
 * without them show the toast in place, so it never hides).
 */
const POPOVERS = typeof HTMLElement !== "undefined" && "showPopover" in HTMLElement.prototype;

/** Shows the popover in the top layer where the browser supports it. */
function showPopover(element: HTMLElement | null): void {
  if (element === null || !POPOVERS) return;
  try {
    element.showPopover();
  } catch {
    // Already shown.
  }
}

/** The toast shown now: the token of the newest one (one place on screen). */
const shown = createStore<{ token: string | null }>()(() => ({ token: null }));

export interface ToastProps {
  kind: ToastKind;
  /** Changes for every new toast (the feature's sequence number). */
  seq: number;
  /** The localized text. */
  text: string;
  /** Undo: its localized label and what it does; no button when omitted. */
  undo?: { label: string; run: () => void } | undefined;
  onDismiss: () => void;
  /** `data-source`: which feature shows it (tests and themes). */
  source?: string | undefined;
  /** `data-action`: what was done (tests and themes). */
  action?: string | undefined;
}

export function Toast({ kind, seq, text, undo, onDismiss, source, action }: ToastProps) {
  const t = useT();
  const ref = useRef<HTMLDivElement>(null);
  const [held, setHeld] = useState(false);
  const token = `${useId()}-${String(seq)}`;

  useLayoutEffect(() => {
    showPopover(ref.current);
  }, [token]);

  // A new toast takes the place; a toast whose place is taken is dismissed.
  // (Passive effects unsubscribe the old toast before the new one claims.)
  useEffect(() => {
    shown.setState({ token });
    return shown.subscribe((s) => {
      if (s.token !== token) onDismiss();
    });
  }, [token, onDismiss]);

  useEffect(() => {
    if (held) return undefined;
    const timer = setTimeout(onDismiss, TOAST_MS[kind]);
    return () => {
      clearTimeout(timer);
    };
  }, [token, kind, held, onDismiss]);

  return (
    <div
      ref={ref}
      popover={POPOVERS ? "manual" : undefined}
      data-part="mail-actions.toast"
      data-kind={kind}
      data-source={source}
      data-action={action}
      role={kind === "error" ? "alert" : "status"}
      onPointerEnter={() => {
        setHeld(true);
      }}
      onPointerLeave={() => {
        setHeld(false);
      }}
      onFocus={() => {
        setHeld(true);
      }}
      onBlur={() => {
        setHeld(false);
      }}
    >
      <span data-part="mail-actions.toast.text">{text}</span>
      {undo && (
        <button type="button" data-part="mail-actions.toast.undo" onClick={undo.run}>
          {undo.label}
        </button>
      )}
      <button
        type="button"
        data-part="mail-actions.toast.dismiss"
        aria-label={t("common-dismiss")}
        title={t("common-dismiss")}
        onClick={onDismiss}
      >
        <Icon name="x" part="mail-actions.toast.dismiss.icon" />
      </button>
    </div>
  );
}
