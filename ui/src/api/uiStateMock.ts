/**
 * The mock core's UI state (M5b, ADR 0109): `GetUiState` / `SetUiState` with
 * the core's checks (`nail_api::ui_state`) and `UiStateChanged`. The real
 * core keeps it in `<config-dir>/ui-state.json`.
 */
import type { JsonValue, Request, Response, UiStateIssue } from "./generated";
import type { MockCore } from "./mockMail";
import { UI_STATE_ISSUE_MESSAGE_IDS } from "./generated";

/** The requests answered here. */
export const UI_STATE_REQUESTS = [
  "GetUiState",
  "SetUiState",
] as const satisfies readonly Request["type"][];

export type UiStateRequest = Extract<Request, { type: (typeof UI_STATE_REQUESTS)[number] }>;

export function isUiStateRequest(request: Request): request is UiStateRequest {
  return (UI_STATE_REQUESTS as readonly string[]).includes(request.type);
}

/** `MAX_UI_STATE_KEY_CHARS`, `MAX_UI_STATE_VALUE_BYTES`, `MAX_UI_STATE_KEYS`. */
export const MAX_UI_STATE_KEY_CHARS = 64;
export const MAX_UI_STATE_VALUE_BYTES = 16 * 1024;
export const MAX_UI_STATE_KEYS = 512;

/** `nail_api::ui_state::is_ui_state_key`. */
export function isUiStateKey(key: string): boolean {
  return (
    key.length > 0 &&
    key.length <= MAX_UI_STATE_KEY_CHARS &&
    key.split(".").every((seg) => /^[a-z][a-z0-9-]*$/.test(seg) && !seg.endsWith("-"))
  );
}

/** `nail_api::ui_state::check_ui_state_write`. */
export function checkUiStateWrite(
  current: Readonly<Record<string, JsonValue>>,
  key: string,
  value: JsonValue | null,
): UiStateIssue | null {
  if (!isUiStateKey(key)) return "invalid-key";
  if (value === null) return null;
  if (new TextEncoder().encode(JSON.stringify(value)).length > MAX_UI_STATE_VALUE_BYTES) {
    return "too-large";
  }
  if (!Object.hasOwn(current, key) && Object.keys(current).length >= MAX_UI_STATE_KEYS) {
    return "too-many-keys";
  }
  return null;
}

const states = new WeakMap<MockCore, Record<string, JsonValue>>();

function stateOf(core: MockCore): Record<string, JsonValue> {
  let state = states.get(core);
  if (state === undefined) {
    state = {};
    states.set(core, state);
  }
  return state;
}

/** Answers a UI-state request like the core. */
export function answerUiState(core: MockCore, request: UiStateRequest): Response {
  const state = stateOf(core);
  if (request.type === "SetUiState") {
    const value = request.value;
    const issue = checkUiStateWrite(state, request.key, value);
    if (issue !== null) {
      return {
        type: "Error",
        code: "invalid_request",
        message: `UI state \`${request.key}\`: ${issue}`,
        localized: { id: UI_STATE_ISSUE_MESSAGE_IDS[issue] },
      };
    }
    const before = Object.hasOwn(state, request.key) ? state[request.key] : null;
    if (JSON.stringify(before) !== JSON.stringify(value)) {
      const rest = Object.entries(state).filter(([k]) => k !== request.key);
      states.set(
        core,
        Object.fromEntries(
          value === null ? rest : [...rest, [request.key, structuredClone(value)]],
        ),
      );
      core.emit({ type: "UiStateChanged", key: request.key, value: structuredClone(value) });
    }
  }
  return { type: "UiState", state: structuredClone(stateOf(core)) };
}
