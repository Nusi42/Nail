/**
 * The mock core's window state and pop-outs (M5c, ADR 0116): the
 * `GetWindowStates` / `SetWindowState` / `ResetWindowStates` checks and
 * events of `nail_api::windows`, and pop-out windows. Like `nail serve`, the
 * mock has no desktop host: `PopOutWindow` is `unsupported` until a test
 * calls `enableMockPopOuts(core)`; `simulatePopOutReport` then plays what
 * the shell reports when the user moves, focuses or closes a pop-out.
 */
import type {
  JsonValue,
  PopOut,
  PopOutChange,
  PopOutIssue,
  Request,
  Response,
  WindowGeometry,
  WindowState,
} from "./generated";
import type { MockCore } from "./mockMail";
import {
  MAX_POP_OUTS,
  checkWindowStateWrite,
  isEmptyWindowState,
  isValidGeometry,
  parseWindowId,
  windowKind,
} from "../state/windows";
import { charCount } from "../text/chars";
import { POP_OUT_ISSUE_MESSAGE_IDS, WINDOW_STATE_ISSUE_MESSAGE_IDS } from "./generated";

/** The requests answered here. */
export const WINDOW_STATE_REQUESTS = [
  "GetWindowStates",
  "SetWindowState",
  "ResetWindowStates",
  "ListPopOuts",
  "PopOutWindow",
  "ReturnPopOut",
  "ClosePopOut",
  "FocusPopOut",
] as const satisfies readonly Request["type"][];

export type WindowStateRequest = Extract<Request, { type: (typeof WINDOW_STATE_REQUESTS)[number] }>;

export function isWindowStateRequest(request: Request): request is WindowStateRequest {
  return (WINDOW_STATE_REQUESTS as readonly string[]).includes(request.type);
}

/** `MAX_POP_OUT_CONTEXT_BYTES`, `MAX_POP_OUT_TITLE_CHARS`. */
export const MAX_POP_OUT_CONTEXT_BYTES = 16 * 1024;
export const MAX_POP_OUT_TITLE_CHARS = 256;

interface MockWindows {
  states: Record<string, WindowState>;
  /** `null` = no desktop host (`unsupported`). */
  popOuts: PopOut[] | null;
}

const cores = new WeakMap<MockCore, MockWindows>();

function windowsOf(core: MockCore): MockWindows {
  let windows = cores.get(core);
  if (windows === undefined) {
    windows = { states: {}, popOuts: null };
    cores.set(core, windows);
  }
  return windows;
}

/** Gives the mock core a desktop host for pop-outs (tests of WP173). */
export function enableMockPopOuts(core: MockCore): void {
  const windows = windowsOf(core);
  windows.popOuts ??= [];
}

/** `check_pop_out`. */
export function checkPopOut(
  openCount: number,
  alreadyOpen: boolean,
  window: string,
  title: string | null,
  context: JsonValue | null,
): PopOutIssue | null {
  if (parseWindowId(window) === null) return "invalid-window";
  if (
    (title !== null && charCount(title) > MAX_POP_OUT_TITLE_CHARS) ||
    (context !== null &&
      new TextEncoder().encode(JSON.stringify(context)).length > MAX_POP_OUT_CONTEXT_BYTES)
  ) {
    return "too-large";
  }
  if (!alreadyOpen && openCount >= MAX_POP_OUTS) return "too-many";
  return null;
}

function error(
  code: "invalid_request" | "conflict" | "unsupported",
  message: string,
  id: string,
): Response {
  return { type: "Error", code, message, localized: { id } };
}

function setState(core: MockCore, kind: string, state: WindowState | null): void {
  const windows = windowsOf(core);
  const before = Object.hasOwn(windows.states, kind) ? windows.states[kind] : null;
  const after = state !== null && !isEmptyWindowState(state) ? state : null;
  if (JSON.stringify(before ?? null) === JSON.stringify(after)) return;
  const rest = Object.entries(windows.states).filter(([k]) => k !== kind);
  windows.states = Object.fromEntries(
    after === null ? rest : [...rest, [kind, structuredClone(after)]],
  );
  core.emit({ type: "WindowStateChanged", window: kind, state: structuredClone(after) });
}

function editState(core: MockCore, window: string, edit: (s: WindowState) => WindowState): void {
  const kind = windowKind(window);
  const windows = windowsOf(core);
  const current = windows.states[kind] ?? { page: null, pop_out: null, popped_out: false };
  setState(core, kind, edit({ ...current }));
}

function changed(core: MockCore, window: string, change: PopOutChange): void {
  core.emit({
    type: "PopOutChanged",
    window,
    change,
    pop_outs: structuredClone(windowsOf(core).popOuts ?? []),
  });
}

function popOutsAnswer(core: MockCore): Response {
  return { type: "PopOuts", pop_outs: structuredClone(windowsOf(core).popOuts ?? []) };
}

function focus(list: PopOut[], window: string): void {
  for (const p of list) p.focused = p.window === window;
}

/** Answers a window request like the core. */
export function answerWindowState(core: MockCore, request: WindowStateRequest): Response {
  const windows = windowsOf(core);
  switch (request.type) {
    case "GetWindowStates":
      break;
    case "SetWindowState": {
      const issue = checkWindowStateWrite(windows.states, request.window, request.state);
      if (issue !== null) {
        return error(
          "invalid_request",
          `window state \`${request.window}\`: ${issue}`,
          WINDOW_STATE_ISSUE_MESSAGE_IDS[issue],
        );
      }
      setState(core, request.window, request.state);
      break;
    }
    case "ResetWindowStates":
      for (const kind of Object.keys(windows.states)) setState(core, kind, null);
      break;
    case "ListPopOuts":
      return popOutsAnswer(core);
    case "PopOutWindow": {
      const list = windows.popOuts;
      if (list === null) {
        return error(
          "unsupported",
          "only the desktop app has pop-out windows",
          "desktop-unavailable",
        );
      }
      const open = list.some((p) => p.window === request.window);
      const issue = checkPopOut(
        list.length,
        open,
        request.window,
        request.title ?? null,
        request.context ?? null,
      );
      if (issue !== null) {
        return error(
          "invalid_request",
          `pop-out \`${request.window}\`: ${issue}`,
          POP_OUT_ISSUE_MESSAGE_IDS[issue],
        );
      }
      if (open) {
        focus(list, request.window);
        changed(core, request.window, "focused");
        return popOutsAnswer(core);
      }
      const geometry = windows.states[windowKind(request.window)]?.pop_out ?? null;
      for (const p of list) p.focused = false;
      list.push({
        window: request.window,
        geometry,
        context: request.context ?? null,
        focused: true,
      });
      editState(core, request.window, (s) => ({ ...s, popped_out: true }));
      changed(core, request.window, "opened");
      return popOutsAnswer(core);
    }
    case "ReturnPopOut":
    case "ClosePopOut":
    case "FocusPopOut": {
      const list = windows.popOuts;
      if (list === null) {
        return error(
          "unsupported",
          "only the desktop app has pop-out windows",
          "desktop-unavailable",
        );
      }
      if (!list.some((p) => p.window === request.window)) {
        return error(
          "conflict",
          `pop-out \`${request.window}\`: not-open`,
          "pop-out-issue-not-open",
        );
      }
      if (request.type === "FocusPopOut") {
        focus(list, request.window);
        changed(core, request.window, "focused");
      } else {
        finish(core, request.window, request.type === "ReturnPopOut" ? "returned" : "closed");
      }
      return popOutsAnswer(core);
    }
  }
  return { type: "WindowStates", states: structuredClone(windowsOf(core).states) };
}

function finish(core: MockCore, window: string, change: "closed" | "returned"): void {
  const windows = windowsOf(core);
  windows.popOuts = (windows.popOuts ?? []).filter((p) => p.window !== window);
  if (change === "returned") editState(core, window, (s) => ({ ...s, popped_out: false }));
  changed(core, window, change);
}

/**
 * Plays a report of the desktop shell (`DesktopChange::PopOut`): the user
 * moved, focused or closed a pop-out. Ignored for windows that are not open.
 */
export function simulatePopOutReport(
  core: MockCore,
  window: string,
  change: PopOutChange,
  geometry: WindowGeometry | null = null,
): void {
  const list = windowsOf(core).popOuts;
  const entry = list?.find((p) => p.window === window);
  if (!list || !entry) return;
  if (change === "closed" || change === "returned") {
    finish(core, window, change);
    return;
  }
  if (change === "focused") focus(list, window);
  if (geometry !== null && isValidGeometry(geometry)) {
    entry.geometry = geometry;
    editState(core, window, (s) => ({ ...s, pop_out: geometry }));
  }
  changed(core, window, change);
}
