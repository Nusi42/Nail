/**
 * The window model of M5c (ADR 0116), mirrored from `nail_api::windows` and
 * tested against the same cases (`windows.test.ts`). Shared contract file:
 * the window manager (WP171, `features/windows/` + `layout/`), the
 * composers (WP172) and the pop-outs (WP173) build on it; change it only
 * through the integration.
 *
 * - **Ids.** A window *kind* is a layout window or dialog name
 *   (kebab-case); a window *id* is the kind or `<kind>:<instance>` (one
 *   composer per draft: `composer:d42`). `UiState.windows` holds ids in
 *   z-order (topmost last = focused).
 * - **Specs.** `resolveWindowSpec`: the defaults (or the confirmation
 *   defaults), then the feature's registration (`Feature.windows`), then
 *   the layout's `window` properties (`ResolvedLayout.window_frames`).
 * - **State.** Remembered per kind in the core (`window-state.json`;
 *   `./windowState.ts`); geometry checks as in the core.
 * - **Pop-outs.** The desktop opens a pop-out at `#/window/<id>`.
 */
import { createContext, useContext } from "react";
import type {
  ResolvedLayout,
  WindowFrame,
  WindowGeometry,
  WindowSpec,
  WindowState,
  WindowStateIssue,
} from "../api/generated";

/** `MAX_WINDOW_STATES`. */
export const MAX_WINDOW_STATES = 256;
/** `MAX_WINDOW_KIND_CHARS`. */
export const MAX_WINDOW_KIND_CHARS = 64;
/** `MAX_WINDOW_INSTANCE_CHARS`. */
export const MAX_WINDOW_INSTANCE_CHARS = 64;
/** `WINDOW_INSTANCE_SEPARATOR`. */
export const WINDOW_INSTANCE_SEPARATOR = ":";
/** `MIN_WINDOW_EDGE`: smallest width / height (and minimum size). */
export const MIN_WINDOW_EDGE = 120;
/** `MAX_WINDOW_EDGE`. */
export const MAX_WINDOW_EDGE = 16_384;
/** `MAX_WINDOW_COORDINATE`. */
export const MAX_WINDOW_COORDINATE = 65_536;
/** `WINDOW_CASCADE_OFFSET`: a new instance's offset from the previous one. */
export const WINDOW_CASCADE_OFFSET = 24;
/** `WINDOW_KEYBOARD_STEP`: arrow-key move / resize step. */
export const WINDOW_KEYBOARD_STEP = 16;
/** `WINDOW_MIN_VISIBLE`: how much of a window always stays on the page. */
export const WINDOW_MIN_VISIBLE = 48;
/** `MAX_POP_OUTS`. */
export const MAX_POP_OUTS = 32;
/** `POP_OUT_ROUTE_PREFIX`. */
export const POP_OUT_ROUTE_PREFIX = "#/window/";
/** `POP_OUT_LABEL_PREFIX` (the OS window label of a pop-out). */
export const POP_OUT_LABEL_PREFIX = "popout-";
/**
 * The DOM event the desktop shell dispatches in a pop-out's page when the
 * user closes its OS window (close button, Alt+F4): the page closes its
 * window like the window's own close button would (app/src-tauri
 * `popout.rs` `POP_OUT_CLOSE_EVENT`).
 */
export const POP_OUT_CLOSE_EVENT = "nail-pop-out-close-requested";

/** `nail_api::windows::is_window_kind`. */
export function isWindowKind(kind: string): boolean {
  return (
    kind.length <= MAX_WINDOW_KIND_CHARS &&
    /^[a-z][a-z0-9-]*$/.test(kind) &&
    !kind.endsWith("-") &&
    !kind.includes("--")
  );
}

/** `nail_api::windows::is_window_instance`. */
export function isWindowInstance(key: string): boolean {
  return key.length > 0 && key.length <= MAX_WINDOW_INSTANCE_CHARS && /^[A-Za-z0-9_-]+$/.test(key);
}

/** A parsed window id (`WindowId`). */
export interface WindowIdParts {
  kind: string;
  /** `null` for the kind's single window. */
  instance: string | null;
}

/** `WindowId::parse`: `null` when `id` is not a window id. */
export function parseWindowId(id: string): WindowIdParts | null {
  const at = id.indexOf(WINDOW_INSTANCE_SEPARATOR);
  const kind = at < 0 ? id : id.slice(0, at);
  const instance = at < 0 ? null : id.slice(at + 1);
  if (!isWindowKind(kind)) return null;
  if (instance !== null && !isWindowInstance(instance)) return null;
  return { kind, instance };
}

/** `window_id`: the kind, or `<kind>:<instance>`. */
export function windowId(kind: string, instance: string | null = null): string {
  return instance === null ? kind : `${kind}${WINDOW_INSTANCE_SEPARATOR}${instance}`;
}

/** `window_kind`: everything before `:` (not validated). */
export function windowKind(id: string): string {
  const at = id.indexOf(WINDOW_INSTANCE_SEPARATOR);
  return at < 0 ? id : id.slice(0, at);
}

/**
 * An instance key not used by any of `openIds` for `kind`: `w1`, `w2`, …
 * (for windows without a natural key; composers use their draft id).
 */
export function newWindowInstance(kind: string, openIds: readonly string[]): string {
  const used = new Set(openIds);
  for (let n = 1; ; n += 1) {
    const id = windowId(kind, `w${String(n)}`);
    if (!used.has(id)) return id;
  }
}

/** `pop_out_route`. */
export function popOutRoute(window: string): string {
  return `${POP_OUT_ROUTE_PREFIX}${window}`;
}

/** `parse_pop_out_route`: the window id of a `location.hash`, if any. */
export function parsePopOutRoute(hash: string): string | null {
  if (!hash.startsWith(POP_OUT_ROUTE_PREFIX)) return null;
  const id = hash.slice(POP_OUT_ROUTE_PREFIX.length);
  return parseWindowId(id) ? id : null;
}

/** `pop_out_label`. */
export function popOutLabel(window: string): string {
  return `${POP_OUT_LABEL_PREFIX}${window}`;
}

/**
 * `DEFAULT_WINDOW_SPEC`. Popping out is opt-in (`pop-out=#true` in
 * `layout.kdl` or `pop_out: true` in the registration): only windows that
 * show core data alone or register a pop-out preparer work in another
 * webview (each has its own UI store).
 */
export const DEFAULT_WINDOW_SPEC: Readonly<WindowSpec> = {
  modal: false,
  resizable: true,
  min_size: { width: 360, height: 240 },
  size: { width: 720, height: 520 },
  pop_out: false,
  instances: "single",
};

/** `CONFIRMATION_WINDOW_SPEC` (the only modal windows). */
export const CONFIRMATION_WINDOW_SPEC: Readonly<WindowSpec> = {
  modal: true,
  resizable: false,
  min_size: { width: 320, height: 160 },
  size: { width: 440, height: 220 },
  pop_out: false,
  instances: "single",
};

/**
 * A feature's registration of a window kind (`Feature.windows`): any spec
 * fields over the defaults; `confirmation: true` starts from
 * `CONFIRMATION_WINDOW_SPEC` (modal) instead.
 */
export interface WindowRegistration extends Partial<WindowSpec> {
  confirmation?: boolean;
}

/** Window registrations by kind (`AppRegistry.windows`). */
export type WindowRegistry = Readonly<Record<string, WindowRegistration>>;

const clampEdge = (v: number) => Math.min(MAX_WINDOW_EDGE, Math.max(MIN_WINDOW_EDGE, v));

/** `WindowSpec::normalized`. */
export function normalizeSpec(spec: WindowSpec): WindowSpec {
  const min_size = {
    width: clampEdge(spec.min_size.width),
    height: clampEdge(spec.min_size.height),
  };
  return {
    ...spec,
    min_size,
    size: {
      width: Math.max(clampEdge(spec.size.width), min_size.width),
      height: Math.max(clampEdge(spec.size.height), min_size.height),
    },
  };
}

/** `WindowSpec::with_frame`. */
export function withFrame(spec: WindowSpec, frame: WindowFrame): WindowSpec {
  return normalizeSpec({
    modal: frame.modal ?? spec.modal,
    resizable: frame.resizable ?? spec.resizable,
    min_size: frame.min_size ?? spec.min_size,
    size: frame.size ?? spec.size,
    pop_out: frame.pop_out ?? spec.pop_out,
    instances: frame.instances ?? spec.instances,
  });
}

/**
 * The spec of a window kind (or id): defaults → registration → the
 * layout's `window` properties (`ResolvedLayout::window_spec`).
 */
export function resolveWindowSpec(
  window: string,
  layout: Pick<ResolvedLayout, "window_frames"> | null,
  registry: WindowRegistry,
): WindowSpec {
  const kind = windowKind(window);
  const registration = Object.hasOwn(registry, kind) ? registry[kind] : undefined;
  const { confirmation, ...fields } = registration ?? {};
  const base: WindowSpec = {
    ...(confirmation ? CONFIRMATION_WINDOW_SPEC : DEFAULT_WINDOW_SPEC),
    ...fields,
  };
  const frames = layout?.window_frames ?? {};
  const frame = Object.hasOwn(frames, kind) ? frames[kind] : undefined;
  return frame ? withFrame(base, frame) : normalizeSpec(base);
}

/** `WindowGeometry::is_valid`. */
export function isValidGeometry(g: WindowGeometry): boolean {
  const edge = (v: number) => Number.isInteger(v) && v >= 1 && v <= MAX_WINDOW_EDGE;
  const coord = (v: number) => Number.isInteger(v) && Math.abs(v) <= MAX_WINDOW_COORDINATE;
  return (
    edge(g.rect.width) &&
    edge(g.rect.height) &&
    coord(g.rect.x) &&
    coord(g.rect.y) &&
    !(g.maximized && g.snap !== null)
  );
}

/** `WindowState::is_empty`. */
export function isEmptyWindowState(state: WindowState): boolean {
  return state.page === null && state.pop_out === null && !state.popped_out;
}

/** `check_window_state_write`. */
export function checkWindowStateWrite(
  current: Readonly<Record<string, WindowState>>,
  window: string,
  state: WindowState | null,
): WindowStateIssue | null {
  if (!isWindowKind(window)) return "invalid-window";
  if (state === null) return null;
  if (
    (state.page !== null && !isValidGeometry(state.page)) ||
    (state.pop_out !== null && !isValidGeometry(state.pop_out))
  ) {
    return "invalid-geometry";
  }
  if (
    !isEmptyWindowState(state) &&
    !Object.hasOwn(current, window) &&
    Object.keys(current).length >= MAX_WINDOW_STATES
  ) {
    return "too-many-windows";
  }
  return null;
}

/** The window a component is rendered in (id = kind or `<kind>:<instance>`). */
export const WindowInstanceContext = createContext<string | null>(null);

/**
 * The window the calling component is rendered in (`null` outside a
 * secondary window, e.g. in the main window). Components that exist once
 * per instance (a composer per draft) key their state by `id`.
 */
export function useWindowInstance(): (WindowIdParts & { id: string }) | null {
  const id = useContext(WindowInstanceContext);
  if (id === null) return null;
  return { id, kind: windowKind(id), instance: parseWindowId(id)?.instance ?? null };
}
