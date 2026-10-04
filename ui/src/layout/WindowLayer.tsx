/**
 * The window manager (M5c WP171, ADR 0116): every secondary window
 * (composer, settings, account setup, outbox, move picker, the feature
 * dialogs) is a window over the main window — moved by its title bar,
 * resized from eight handles, maximized, snapped to a page half, moved and
 * resized from the keyboard, and remembered per kind
 * (`<config-dir>/window-state.json` through `../state/windowState.ts`).
 * A window's content is the layout's `window "<kind>"` tree, or the
 * feature dialog of that name.
 *
 * - **Ids and z-order:** `UiState.windows` (a kind or `<kind>:<instance>`,
 *   topmost = focused last). Clicking or focusing a window raises it
 *   (`focusWindow`); the DOM order stays the opening order, so raising never
 *   remounts a window (editors and frames keep their state); the stacking
 *   order is a `z-index` in each window's geometry rule.
 * - **Modality:** windows are non-modal (`resolveWindowSpec`): the layer
 *   lets the pointer through to the main window around them. A modal
 *   window (a confirmation) blocks only its own flow (`modalOwner`): the
 *   window it belongs to (`composer-close:<key>` → `composer:<key>`, else
 *   the window that had the focus when it opened) becomes `inert`, or —
 *   when it was opened from the main window — the main window is covered
 *   by a scrim below every window and focus that strays there comes back.
 *   Other windows stay usable.
 * - **Keys:** Ctrl+F6 goes to the next window (from the main window to the
 *   topmost one; with one window open back and forth), Ctrl+Shift+F6 back to
 *   the main window; inside a window Tab stays in it and Escape (the
 *   layout's `window.close` binding) closes it.
 * - **Placement:** a new window takes the first cascade slot no open
 *   window of its kind uses (`cascadeSlots`); a confirmation that belongs
 *   to a window opens centred over it and stacks just above it (the user
 *   can raise another window over both), a confirmation of the main window
 *   stays above every window.
 * - Popped-out windows (desktop, `usePopOuts`) are not shown in the page.
 *   In a pop-out page the layer shows the windows the popped-out window
 *   opens (its confirmations); the pop-out itself (`main`) is the main
 *   window there.
 */
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { ResolvedLayout, WindowGeometry, WindowRect } from "../api/generated";
import { setWindowCommandHandler, type WindowCommand } from "../features/windows/commands";
import { cascadeSlots, sameGeometry } from "../features/windows/geometry";
import { useViewport } from "../features/windows/useViewport";
import { useT } from "../i18n/I18nProvider";
import { useUi, useUiStoreApi, type UiState } from "../state/uiStore";
import { usePopOuts, useSetWindowState, useWindowStates } from "../state/windowState";
import {
  WindowInstanceContext,
  parseWindowId,
  resolveWindowSpec,
  windowKind,
} from "../state/windows";
import { useGeometry } from "../theme/ThemeProvider";
import { WindowView } from "./LayoutRenderer";
import { WindowFrame, tabbables, trapTab, type WindowFrameHandle } from "./WindowFrame";
import { useDialogRegistry } from "./dialogRegistry";
import { useWindowSpecs, useWindowTitles } from "./windowSpecs";
import { nonEmpty } from "../text/empty";

export { tabbables, trapTab };

/** Fluent id of a window's title: `window-<name>`. */
export function windowTitleId(name: string): string {
  return `window-${name}`;
}

/**
 * The windows in the order they opened: `previous` without the closed ones,
 * then the new ones (`previous` itself when nothing changed).
 */
export function openingOrder(
  previous: readonly string[],
  shown: readonly string[],
): readonly string[] {
  const present = new Set(shown);
  const kept = previous.filter((id) => present.has(id));
  const added = shown.filter((id) => !previous.includes(id));
  return kept.length === previous.length && added.length === 0 ? previous : [...kept, ...added];
}

/** The stacking order of the window at z-order position `index`. */
const zOf = (index: number) => 2 * index + 2;

/** Joins window titles (never part of one). */
const TITLE_SEPARATOR = String.fromCharCode(0);

/** The scrim over the main window: below every window. */
const SCRIM_Z = 1;

/** What a modal window blocks: the main window. */
export const MAIN_OWNER = "main";

/**
 * The flow a modal window blocks: an open non-modal window with the same
 * instance key (`composer-close:w1` → `composer:w1`), else `focused` (the
 * window that had the focus when the modal opened, if it is still open
 * and not modal), else the main window (`MAIN_OWNER`).
 */
export function modalOwner(
  modal: string,
  open: readonly string[],
  isModal: (id: string) => boolean,
  focused: string | null,
): string {
  const parts = parseWindowId(modal);
  if (parts?.instance) {
    const sibling = open.find(
      (id) =>
        id !== modal &&
        !isModal(id) &&
        parseWindowId(id)?.instance === parts.instance &&
        windowKind(id) !== parts.kind,
    );
    if (sibling !== undefined) return sibling;
  }
  if (focused !== null && focused !== modal && open.includes(focused) && !isModal(focused)) {
    return focused;
  }
  return MAIN_OWNER;
}

/**
 * The stacking order of every shown window (`shown` = z-order, topmost
 * last): non-modal windows by position; a modal window that belongs to an
 * open window just above that window (another window raised over the
 * owner covers its confirmation too); a modal window of the main window
 * above every window.
 */
export function stackingOrder(
  shown: readonly string[],
  isModal: (id: string) => boolean,
  ownerOf: (id: string) => string,
): ReadonlyMap<string, number> {
  const out = new Map<string, number>();
  shown.forEach((id, index) => {
    if (!isModal(id)) {
      out.set(id, zOf(index));
      return;
    }
    const owner = ownerOf(id);
    const at = owner === MAIN_OWNER ? -1 : shown.indexOf(owner);
    out.set(id, at < 0 ? zOf(shown.length + index) : zOf(at) + 1);
  });
  return out;
}

/**
 * Renders every open secondary window (`main`: in a pop-out page, the
 * popped-out window the page itself shows).
 */
export function WindowLayer({ layout, main }: { layout: ResolvedLayout; main?: string }) {
  const t = useT();
  const store = useUiStoreApi();
  const windows = useUi((s) => s.windows);
  const focusWindow = useUi((s) => s.focusWindow);
  const closeWindow = useUi((s) => s.closeWindow);
  const specs = useWindowSpecs();
  const titleFns = useWindowTitles();
  const dialogs = useDialogRegistry();
  const viewport = useViewport();
  const states = useWindowStates();
  const popOuts = usePopOuts().data;
  const setWindowState = useSetWindowState();
  const frames = useRef(new Map<string, WindowFrameHandle>());
  const layerRef = useRef<HTMLDivElement>(null);
  const mainFocus = useRef<HTMLElement | null>(null);

  // Popped-out windows live in their own OS window.
  const shown = useMemo(() => {
    const out = new Set((popOuts ?? []).map((p) => p.window));
    if (main !== undefined) out.add(main);
    return windows.filter((id) => !out.has(id));
  }, [windows, popOuts, main]);

  // Titles from the UI state (a composer's subject), one string so the
  // selector result compares by value; NUL never occurs in a title.
  const titleKey = useUi(
    useCallback(
      (s: UiState) =>
        shown.map((id) => titleFns[windowKind(id)]?.(s, id) ?? "").join(TITLE_SEPARATOR),
      [shown, titleFns],
    ),
  );
  const customTitles = titleKey.split(TITLE_SEPARATOR);

  // Opening order (stable DOM order), adjusted while rendering.
  const [order, setOrder] = useState<readonly string[]>([]);
  const nextOrder = openingOrder(order, shown);
  if (nextOrder !== order) setOrder(nextOrder);
  // Cascade slots (kept per window; a new one takes the first free slot).
  const [slots, setSlots] = useState<ReadonlyMap<string, number>>(() => new Map());
  const nextSlots = cascadeSlots(slots, nextOrder, windowKind);
  if (nextSlots !== slots) setSlots(nextSlots);

  const specOf = useCallback((id: string) => resolveWindowSpec(id, layout, specs), [layout, specs]);

  // ---- Modality: each modal window blocks its owner only ----
  // The window that has the focus (`null` = the main window; modal windows
  // are not counted), kept current by the focus listener below and read
  // when a modal window first shows.
  const focusedWindow = useRef<string | null>(null);
  const modals = useMemo(() => shown.filter((id) => specOf(id).modal), [shown, specOf]);
  const [owners, setOwners] = useState<ReadonlyMap<string, string>>(() => new Map());
  useLayoutEffect(() => {
    setOwners((previous) => {
      const next = new Map<string, string>();
      for (const id of modals) {
        next.set(
          id,
          previous.get(id) ?? modalOwner(id, shown, (w) => specOf(w).modal, focusedWindow.current),
        );
      }
      const same =
        next.size === previous.size && [...next].every(([id, o]) => previous.get(id) === o);
      return same ? previous : next;
    });
  }, [modals, shown, specOf]);
  // Where a window's confirmation opens: over the owner's shown rectangle
  // (taken once, when the owner is known).
  const [anchors, setAnchors] = useState<ReadonlyMap<string, WindowRect>>(() => new Map());
  useLayoutEffect(() => {
    setAnchors((previous) => {
      const next = new Map<string, WindowRect>();
      for (const [id, owner] of owners) {
        if (owner === MAIN_OWNER) continue;
        const rect = previous.get(id) ?? frames.current.get(owner)?.rect();
        if (rect !== undefined) next.set(id, rect);
      }
      const same =
        next.size === previous.size && [...next].every(([id, r]) => previous.get(id) === r);
      return same ? previous : next;
    });
  }, [owners]);
  // Until its owner is known (the first layout pass), a modal window blocks the page.
  const ownerOf = (id: string): string => owners.get(id) ?? MAIN_OWNER;
  const blockedWindows = new Set(modals.map(ownerOf).filter((o) => o !== MAIN_OWNER));
  const mainBlocked = modals.some((id) => ownerOf(id) === MAIN_OWNER);
  // The newest modal window that blocks the main window (focus returns there).
  const pageModal = modals.findLast((id) => ownerOf(id) === MAIN_OWNER) ?? null;
  useGeometry("wm-scrim", mainBlocked ? { "z-index": String(SCRIM_Z) } : {});

  // ---- Remembering ----
  const statesRef = useRef(states.data);
  useLayoutEffect(() => {
    statesRef.current = states.data;
  });
  const mutate = setWindowState.mutate;
  const remember = useCallback(
    (kind: string, geometry: WindowGeometry) => {
      const current = statesRef.current?.[kind] ?? null;
      if (sameGeometry(current?.page ?? null, geometry)) return;
      mutate({
        window: kind,
        state: {
          page: geometry,
          pop_out: current?.pop_out ?? null,
          popped_out: current?.popped_out ?? false,
        },
      });
    },
    [mutate],
  );
  const loaded = states.isSuccess || states.isError;

  // ---- Focus: the main window's last focus, modal guard ----
  const shownRef = useRef(shown);
  const modalRef = useRef<string | null>(null);
  const blockedRef = useRef<ReadonlySet<string>>(blockedWindows);
  useLayoutEffect(() => {
    shownRef.current = shown;
    modalRef.current = pageModal;
    blockedRef.current = blockedWindows;
  });
  useEffect(() => {
    const onFocusIn = (event: FocusEvent) => {
      const target = event.target instanceof HTMLElement ? event.target : null;
      if (target === null) return;
      if (!layerRef.current?.contains(target)) {
        const modal = modalRef.current;
        // Focus strayed into the main window behind a modal window (menus
        // and pop-ups are windows of their own).
        if (modal !== null && !target.closest("[data-key-modal]")) {
          frames.current.get(modal)?.focus();
          return;
        }
        mainFocus.current = target;
        focusedWindow.current = null;
      } else {
        const frame = target.closest<HTMLElement>('[data-part="window"]');
        if (frame !== null && !frame.hasAttribute("data-modal")) {
          focusedWindow.current = frame.getAttribute("data-window");
        }
      }
    };
    document.addEventListener("focusin", onFocusIn);
    return () => {
      document.removeEventListener("focusin", onFocusIn);
    };
  }, []);

  const focusMain = useCallback(() => {
    const previous = mainFocus.current;
    if (previous?.isConnected && !previous.closest("[inert]")) {
      previous.focus();
      return;
    }
    const app = layerRef.current?.closest<HTMLElement>('[data-part="app"]') ?? document.body;
    const first = tabbables(app).find((el) => !layerRef.current?.contains(el));
    first?.focus();
  }, []);

  const raiseAndFocus = useCallback(
    (id: string) => {
      focusWindow(id);
      frames.current.get(id)?.focus();
    },
    [focusWindow],
  );

  // ---- Commands (Ctrl+F6, actions) ----
  const run = useCallback(
    (command: WindowCommand) => {
      const ids = shownRef.current;
      const top = ids[ids.length - 1];
      if (command.type === "reset") {
        for (const frame of frames.current.values()) frame.reset();
        return;
      }
      if (top === undefined) return;
      if (command.type === "maximize") {
        frames.current.get(top)?.toggleMaximize();
        return;
      }
      // Windows whose confirmation is open, and the main window behind a
      // modal window, are skipped.
      const mainBlocked = modalRef.current !== null;
      const usable = ids.filter((id) => !blockedRef.current.has(id));
      if (command.type === "main") {
        if (!mainBlocked) focusMain();
        return;
      }
      const active = document.activeElement;
      const inWindow = active instanceof HTMLElement && layerRef.current?.contains(active);
      const usableTop = usable[usable.length - 1];
      if (usableTop === undefined) return;
      if (!inWindow) raiseAndFocus(usableTop);
      else if (usable.length === 1) {
        if (!mainBlocked) focusMain();
      }
      // The bottom window comes up: every window comes round in turn.
      else raiseAndFocus(usable[0] ?? usableTop);
    },
    [focusMain, raiseAndFocus],
  );
  useEffect(() => {
    setWindowCommandHandler(store, run);
    return () => {
      setWindowCommandHandler(store, null);
    };
  }, [store, run]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "F6" || !event.ctrlKey || event.altKey || event.metaKey) return;
      if (shownRef.current.length === 0) return;
      event.preventDefault();
      run({ type: event.shiftKey ? "main" : "cycle" });
    };
    document.addEventListener("keydown", onKeyDown, true);
    return () => {
      document.removeEventListener("keydown", onKeyDown, true);
    };
  }, [run]);

  if (shown.length === 0) return null;

  const stacking = stackingOrder(shown, (id) => specOf(id).modal, ownerOf);
  return (
    <div ref={layerRef} data-part="window-layer" data-modal={mainBlocked ? "" : undefined}>
      {mainBlocked ? (
        <div
          data-part="window-scrim"
          data-geom="wm-scrim"
          aria-hidden="true"
          onPointerDown={(event) => {
            // Keep focus in the modal window.
            event.preventDefault();
          }}
        />
      ) : null}
      {nextOrder.map((id) => {
        const kind = windowKind(id);
        const spec = specOf(id);
        const index = shown.indexOf(id);
        const z = stacking.get(id) ?? zOf(index);
        const cascade = nextSlots.get(id) ?? 0;
        // A layout window wins; a feature dialog fills in for a name the layout lacks.
        const Dialog = Object.hasOwn(layout.windows, kind) ? undefined : dialogs[kind];
        const remembered = states.data?.[kind]?.page ?? null;
        return (
          <WindowFrame
            key={id}
            id={id}
            kind={kind}
            spec={spec}
            z={z}
            active={index === shown.length - 1}
            blocked={blockedWindows.has(id)}
            owner={spec.modal ? ownerOf(id) : null}
            viewport={viewport}
            cascade={cascade}
            anchor={spec.modal ? (anchors.get(id) ?? null) : null}
            remembered={remembered}
            loaded={loaded}
            title={nonEmpty(customTitles[index]) ?? t(windowTitleId(kind))}
            onRemember={remember}
            onClose={() => {
              closeWindow(id);
            }}
            onRaise={() => {
              focusWindow(id);
            }}
            handle={(frame: WindowFrameHandle | null) => {
              if (frame === null) frames.current.delete(id);
              else frames.current.set(id, frame);
            }}
          >
            <WindowInstanceContext.Provider value={id}>
              {Dialog ? (
                <Dialog
                  name={id}
                  close={() => {
                    closeWindow(id);
                  }}
                />
              ) : (
                <WindowView layout={layout} name={kind} />
              )}
            </WindowInstanceContext.Provider>
          </WindowFrame>
        );
      })}
    </div>
  );
}
