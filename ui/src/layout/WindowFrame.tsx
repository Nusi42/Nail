/**
 * One window of the window manager (M5c WP171, ADR 0116): the frame
 * (`window`, `role="dialog"`), its title bar (`window.header`) with the
 * title, maximize / restore and close, the eight resize handles
 * (`window.handle`, `data-edge`) and, while a window is dragged to a page
 * edge, the snap preview (`window-snap-preview`, `data-snap`).
 *
 * - **Move:** drag the title bar (pointer capture); at least
 *   `WINDOW_MIN_VISIBLE` px stay on the page. A maximized or snapped window
 *   comes loose at its restored size under the pointer.
 * - **Resize:** the eight handles, never below the minimum size.
 * - **Maximize / restore:** the button or a double-click on the title bar.
 * - **Snap:** drop the window with the pointer at the left / right page
 *   edge to fill that half.
 * - **Keyboard:** on the focused title bar the arrow keys move the window,
 *   Shift+arrow keys resize it; Alt+F7 / Alt+F8 anywhere in the window start
 *   move / size mode (arrow keys, Enter keeps, Escape goes back).
 * - **Remembered:** every change the user makes is handed to `onRemember`
 *   after `SAVE_DELAY_MS` (and when the window closes).
 *
 * Geometry is a CSS geometry rule (`useGeometry`), never an inline style.
 * The theme may still give `window` a width / height: the rule pins the
 * size with min- / max- properties too, so the window manager's size wins.
 */
import {
  useCallback,
  useEffect,
  useImperativeHandle,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type PointerEvent,
  type ReactNode,
  type Ref,
} from "react";
import type { WindowGeometry, WindowRect, WindowSnap, WindowSpec } from "../api/generated";
import {
  DRAG_THRESHOLD,
  RESIZE_EDGES,
  SAVE_DELAY_MS,
  centredOn,
  initialGeometry,
  isArrowKey,
  keyboardMove,
  keyboardResize,
  moveRect,
  looseRect,
  normal,
  resizeRect,
  shownRect,
  snapTarget,
  type ResizeEdge,
  type Viewport,
} from "../features/windows/geometry";
import { PopOutButton } from "../features/popout/PopOutButton";
import { useT } from "../i18n/I18nProvider";
import { useGeometry } from "../theme/ThemeProvider";
import { Icon } from "../widgets/Icon";

const FOCUSABLE =
  'input:not([disabled]), textarea:not([disabled]), select:not([disabled]), button:not([disabled]), [tabindex="0"]';

/** What the window layer can ask of a window. */
export interface WindowFrameHandle {
  /** Maximize or restore (resizable windows only). */
  toggleMaximize: () => void;
  /** Back to the default place (remembered positions were forgotten). */
  reset: () => void;
  /** Focus the window (its last focused element, else the title bar). */
  focus: () => void;
  /** Where the window is shown (page pixels). */
  rect: () => WindowRect;
}

/** Keyboard move / size mode (Alt+F7 / Alt+F8). */
type Mode = "move" | "size";

interface Drag {
  kind: "move" | "resize";
  edge: ResizeEdge | null;
  pointerId: number;
  grab: { x: number; y: number };
  start: WindowGeometry;
  /** The rectangle shown when the drag started. */
  shown: ReturnType<typeof shownRect>;
  /** The pointer moved far enough to count as a drag. */
  moved: boolean;
  /** The geometry of the last pointer move. */
  last: WindowGeometry;
}

export interface WindowFrameProps {
  /** Window id (`<kind>` or `<kind>:<instance>`). */
  id: string;
  /** Window kind (title, remembered state). */
  kind: string;
  spec: WindowSpec;
  /** Stacking order: higher is in front. */
  z: number;
  /** The focused (topmost) window. */
  active: boolean;
  /** Its modal confirmation is open: shown but not usable. */
  blocked: boolean;
  /**
   * For a modal window: what it blocks — `"main"` (the main window; the
   * page behind is covered) or the id of the window it belongs to.
   */
  owner?: string | null;
  viewport: Viewport;
  /** The window's cascade slot (steps down and right from the default place). */
  cascade: number;
  /**
   * The shown rectangle of the window this one belongs to (a confirmation
   * of a composer): it opens centred over it instead of on the page.
   */
  anchor?: WindowRect | null;
  /** The remembered in-page geometry of the kind. */
  remembered: WindowGeometry | null;
  /** Whether the remembered states have loaded (they may arrive after the window opened). */
  loaded: boolean;
  /** Title text. */
  title: string;
  onRemember: (kind: string, geometry: WindowGeometry) => void;
  onClose: () => void;
  onRaise: () => void;
  handle?: Ref<WindowFrameHandle>;
  children: ReactNode;
}

/** `[A-Za-z0-9_-]` only, for element ids. */
const domId = (id: string) => id.replace(/[^A-Za-z0-9_-]/g, "-");

export function WindowFrame({
  id,
  kind,
  spec,
  z,
  active,
  blocked,
  owner = null,
  viewport,
  cascade,
  anchor = null,
  remembered,
  loaded,
  title,
  onRemember,
  onClose,
  onRaise,
  handle,
  children,
}: WindowFrameProps) {
  const t = useT();
  const ref = useRef<HTMLDivElement>(null);
  const headerRef = useRef<HTMLDivElement>(null);
  const min = spec.min_size;
  const [geometry, setGeometry] = useState<WindowGeometry>(() =>
    anchor !== null
      ? normal(centredOn(anchor, spec.size, min, viewport))
      : initialGeometry(loaded ? remembered : null, spec.size, min, viewport, cascade),
  );
  // The remembered state was taken into account (it may load after the window
  // opened); a change by the user settles it too. A window over its owner
  // opens there, whatever is remembered.
  const [settled, setSettled] = useState(loaded || anchor !== null);
  if (!settled && loaded) {
    setSettled(true);
    if (remembered !== null) {
      setGeometry(initialGeometry(remembered, spec.size, min, viewport, cascade));
    }
  }
  // The owner may be known only after the first render: the window moves
  // over it then, unless the user placed it already.
  const [anchoredTo, setAnchoredTo] = useState<WindowRect | null>(anchor);
  const [touched, setTouched] = useState(false);
  if (anchor !== null && anchoredTo === null && !touched) {
    setAnchoredTo(anchor);
    setSettled(true);
    setGeometry(normal(centredOn(anchor, spec.size, min, viewport)));
  }
  const [preview, setPreview] = useState<WindowSnap | null>(null);
  const [mode, setMode] = useState<Mode | null>(null);
  const modeStart = useRef<WindowGeometry | null>(null);
  const [dragging, setDragging] = useState<"move" | "resize" | null>(null);
  const [status, setStatus] = useState("");
  const drag = useRef<Drag | null>(null);
  const lastFocused = useRef<HTMLElement | null>(null);

  // ---- Remembering (debounced; flushed when the window closes) ----
  const pending = useRef<WindowGeometry | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const rememberRef = useRef(onRemember);
  useLayoutEffect(() => {
    rememberRef.current = onRemember;
  });
  const flush = useCallback(() => {
    if (timer.current !== null) clearTimeout(timer.current);
    timer.current = null;
    const next = pending.current;
    pending.current = null;
    if (next !== null) rememberRef.current(kind, next);
  }, [kind]);
  useEffect(() => flush, [flush]);

  /** Applies a change the user made and remembers it shortly after. */
  const commit = useCallback(
    (next: WindowGeometry) => {
      setSettled(true);
      setTouched(true);
      setGeometry(next);
      pending.current = next;
      if (timer.current !== null) clearTimeout(timer.current);
      timer.current = setTimeout(flush, SAVE_DELAY_MS);
    },
    [flush],
  );

  const shown = shownRect(geometry, min, viewport);
  const shownRef = useRef(shown);
  useLayoutEffect(() => {
    shownRef.current = shown;
  });
  const geom = `wm-${id}`;
  useGeometry(geom, {
    left: `${String(shown.x)}px`,
    top: `${String(shown.y)}px`,
    width: `${String(shown.width)}px`,
    "min-width": `${String(shown.width)}px`,
    "max-width": `${String(shown.width)}px`,
    height: `${String(shown.height)}px`,
    "min-height": `${String(shown.height)}px`,
    "max-height": `${String(shown.height)}px`,
    "z-index": String(z),
  });

  const toggleMaximize = useCallback(() => {
    if (!spec.resizable) return;
    const next: WindowGeometry = geometry.maximized
      ? { ...geometry, maximized: false }
      : { rect: geometry.rect, maximized: true, snap: null };
    commit(next);
    setStatus(t(next.maximized ? "window-manager-maximized" : "window-manager-restored"));
  }, [spec.resizable, geometry, commit, t]);

  const focusWindow = useCallback(() => {
    const el = ref.current;
    if (!el) return;
    const target =
      lastFocused.current?.isConnected && el.contains(lastFocused.current)
        ? lastFocused.current
        : headerRef.current;
    target?.focus();
  }, []);

  useImperativeHandle(
    handle,
    () => ({
      toggleMaximize,
      reset: () => {
        pending.current = null;
        if (timer.current !== null) clearTimeout(timer.current);
        timer.current = null;
        setGeometry(initialGeometry(null, spec.size, min, viewport, cascade));
      },
      focus: focusWindow,
      rect: () => shownRef.current,
    }),
    [toggleMaximize, focusWindow, spec.size, min, viewport, cascade],
  );

  // Focus the first field when the window opens on top (of several opening
  // together only the topmost); give focus back on close.
  const openedOnTop = useRef(active);
  useEffect(() => {
    if (!openedOnTop.current) return;
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const el = ref.current;
    const body = el?.querySelector('[data-part="window.body"]');
    const first = body?.querySelector<HTMLElement>(FOCUSABLE) ?? el;
    first?.focus();
    return () => {
      if (previous?.isConnected) previous.focus();
    };
  }, []);

  /** The window restored at its shown place (keyboard steps start there). */
  const restoredShown = () => shownRect(normal(geometry.rect), min, viewport);

  // ---- Pointer: move and resize ----
  const startDrag = (
    event: PointerEvent<HTMLElement>,
    kindOfDrag: "move" | "resize",
    edge: ResizeEdge | null,
  ) => {
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = {
      kind: kindOfDrag,
      edge,
      pointerId: event.pointerId,
      grab: { x: event.clientX, y: event.clientY },
      start: geometry,
      shown,
      moved: false,
      last: geometry,
    };
  };

  const onHeaderPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0 || blocked) return;
    if (event.target instanceof Element && event.target.closest("button")) return;
    onRaise();
    startDrag(event, "move", null);
    headerRef.current?.focus({ preventScroll: true });
  };

  const onPointerMove = (event: PointerEvent<HTMLElement>) => {
    const state = drag.current;
    if (state?.pointerId !== event.pointerId) return;
    const dx = event.clientX - state.grab.x;
    const dy = event.clientY - state.grab.y;
    if (!state.moved) {
      if (Math.abs(dx) + Math.abs(dy) < DRAG_THRESHOLD) return;
      state.moved = true;
      setDragging(state.kind);
    }
    if (state.kind === "resize" && state.edge !== null) {
      state.last = normal(resizeRect(state.shown, state.edge, dx, dy, min, viewport));
      setGeometry(state.last);
      return;
    }
    const pointer = { x: event.clientX, y: event.clientY };
    const loose = state.start.maximized || state.start.snap !== null;
    const rect = loose
      ? looseRect(state.shown, state.start.rect, state.grab, pointer, min, viewport)
      : moveRect(state.shown, dx, dy, viewport);
    state.last = normal(rect);
    setGeometry(state.last);
    setPreview(spec.resizable ? snapTarget(event.clientX, viewport) : null);
  };

  const endDrag = (event: PointerEvent<HTMLElement>, cancelled: boolean) => {
    const state = drag.current;
    if (state?.pointerId !== event.pointerId) return;
    drag.current = null;
    event.currentTarget.releasePointerCapture(event.pointerId);
    setDragging(null);
    const snap =
      state.kind === "move" && spec.resizable ? snapTarget(event.clientX, viewport) : null;
    setPreview(null);
    if (!state.moved) return;
    if (cancelled) {
      setGeometry(state.start);
      return;
    }
    if (snap !== null) {
      commit({ rect: state.start.rect, maximized: false, snap });
      setStatus(t(snap === "left" ? "window-snap-left" : "window-snap-right"));
    } else {
      commit(state.last);
    }
  };

  // ---- Keyboard ----
  const step = (key: Parameters<typeof keyboardMove>[1], resize: boolean) => {
    const base = restoredShown();
    const rect = resize
      ? keyboardResize(base, key, min, viewport)
      : keyboardMove(base, key, viewport);
    commit(normal(rect));
  };

  const endMode = (keep: boolean) => {
    if (!keep && modeStart.current !== null) commit(modeStart.current);
    modeStart.current = null;
    setMode(null);
    setStatus("");
  };

  const startMode = (next: Mode) => {
    if (next === "size" && !spec.resizable) return;
    modeStart.current = geometry;
    setMode(next);
    setStatus(t(next === "move" ? "window-manager-moving" : "window-manager-sizing"));
    headerRef.current?.focus();
  };

  const onHeaderKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.target !== event.currentTarget || event.altKey || event.ctrlKey || event.metaKey) {
      return;
    }
    if (mode !== null) {
      if (isArrowKey(event.key)) {
        event.preventDefault();
        event.stopPropagation();
        step(event.key, mode === "size");
      } else if (event.key === "Enter" || event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        endMode(event.key === "Enter");
      }
      return;
    }
    if (isArrowKey(event.key)) {
      if (event.shiftKey && !spec.resizable) return;
      event.preventDefault();
      event.stopPropagation();
      step(event.key, event.shiftKey);
    }
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.altKey && !event.ctrlKey && !event.metaKey && !event.shiftKey) {
      if (event.key === "F7" || event.key === "F8") {
        event.preventDefault();
        event.stopPropagation();
        startMode(event.key === "F7" ? "move" : "size");
        return;
      }
    }
    if (ref.current) trapTab(event, ref.current);
  };

  const titleId = `nail-window-${domId(id)}-title`;
  const hintId = `nail-window-${domId(id)}-hint`;
  const states = [
    active ? "active" : null,
    geometry.maximized ? "maximized" : null,
    !geometry.maximized && geometry.snap !== null ? "snapped" : null,
  ].filter((s): s is string => s !== null);
  const handles = spec.resizable && !geometry.maximized;

  return (
    <>
      {preview !== null ? (
        <div data-part="window-snap-preview" data-snap={preview} aria-hidden="true" />
      ) : null}
      {/* eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions -- the window raises on any pointer or focus inside it; its keys arrive from the controls inside. */}
      <div
        ref={ref}
        data-part="window"
        data-window={id}
        data-geom={geom}
        data-state={states.length > 0 ? states.join(" ") : undefined}
        data-snap={!geometry.maximized && geometry.snap !== null ? geometry.snap : undefined}
        data-modal={spec.modal ? "" : undefined}
        data-owner={spec.modal && owner !== null ? owner : undefined}
        data-blocked={blocked ? "" : undefined}
        data-mode={mode ?? undefined}
        data-dragging={dragging ?? undefined}
        data-key-context="window"
        data-key-modal=""
        role="dialog"
        // Modal for assistive technology only where it blocks the main
        // window; a confirmation of one window leaves the others usable.
        aria-modal={spec.modal && owner === "main" ? true : undefined}
        aria-labelledby={titleId}
        tabIndex={-1}
        inert={blocked}
        onPointerDown={onRaise}
        onFocus={(event) => {
          if (event.target instanceof HTMLElement) lastFocused.current = event.target;
          onRaise();
        }}
        onKeyDown={onKeyDown}
      >
        {/* eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions -- the title bar moves the window by pointer drag and by keyboard. */}
        <div
          ref={headerRef}
          data-part="window.header"
          role="group"
          aria-label={t("window-manager-title-bar", { title })}
          aria-describedby={hintId}
          // eslint-disable-next-line jsx-a11y/no-noninteractive-tabindex -- the title bar is focusable so the window can be moved and resized with the keyboard.
          tabIndex={0}
          onPointerDown={onHeaderPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={(event) => {
            endDrag(event, false);
          }}
          onPointerCancel={(event) => {
            endDrag(event, true);
          }}
          onDoubleClick={(event) => {
            if (event.target instanceof Element && event.target.closest("button")) return;
            toggleMaximize();
          }}
          onKeyDown={onHeaderKeyDown}
          onBlur={(event) => {
            if (mode !== null && event.relatedTarget !== null) endMode(true);
          }}
        >
          <h2 data-part="window.title" id={titleId}>
            {title}
          </h2>
          <span data-part="window.hint" id={hintId}>
            {t(spec.resizable ? "window-manager-hint" : "window-manager-hint-fixed")}
          </span>
          {spec.resizable ? (
            <button
              type="button"
              data-part="window.maximize"
              aria-label={t(
                geometry.maximized ? "window-manager-restore" : "window-manager-maximize",
              )}
              title={t(geometry.maximized ? "window-manager-restore" : "window-manager-maximize")}
              onClick={toggleMaximize}
            >
              <Icon
                name={geometry.maximized ? "minimize-2" : "maximize-2"}
                part="window.maximize.icon"
              />
            </button>
          ) : null}
          <PopOutButton window={id} spec={spec} />
          <button
            type="button"
            data-part="window.close"
            aria-label={t("window-close")}
            title={t("window-close")}
            onClick={onClose}
          >
            <Icon name="x" part="window.close.icon" />
          </button>
        </div>
        <div data-part="window.body">{children}</div>
        <span data-part="window.status" aria-live="polite" aria-atomic="true">
          {status}
        </span>
        {handles
          ? RESIZE_EDGES.map((edge) => (
              <div
                key={edge}
                data-part="window.handle"
                data-edge={edge}
                aria-hidden="true"
                onPointerDown={(event) => {
                  if (event.button !== 0 || blocked) return;
                  onRaise();
                  startDrag(event, "resize", edge);
                }}
                onPointerMove={onPointerMove}
                onPointerUp={(event) => {
                  endDrag(event, false);
                }}
                onPointerCancel={(event) => {
                  endDrag(event, true);
                }}
              />
            ))
          : null}
      </div>
    </>
  );
}

/** Elements Tab can reach inside `root`, in order (not inside hidden panels). */
export function tabbables(root: HTMLElement): HTMLElement[] {
  const all = root.querySelectorAll<HTMLElement>(
    `${FOCUSABLE}, a[href], [contenteditable="true"], iframe`,
  );
  return [...all].filter(
    (el) =>
      !el.closest("[hidden]") &&
      !el.closest("[inert]") &&
      el.tabIndex >= 0 &&
      !(el instanceof HTMLInputElement && el.type === "hidden"),
  );
}

/**
 * Keeps Tab / Shift+Tab inside a window: from the last element to the
 * first and back (WAI-ARIA dialog pattern; like an OS window, Ctrl+F6
 * leaves it).
 */
export function trapTab(event: KeyboardEvent<HTMLElement>, root: HTMLElement): void {
  if (event.key !== "Tab" || event.altKey || event.ctrlKey || event.metaKey) return;
  const items = tabbables(root);
  const first = items[0];
  const last = items[items.length - 1];
  if (!first || !last) {
    event.preventDefault();
    root.focus();
    return;
  }
  const active = root.ownerDocument.activeElement;
  if (event.shiftKey && (active === first || active === root)) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && active === last) {
    event.preventDefault();
    first.focus();
  }
}
