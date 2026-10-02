/**
 * The mock core's answers to the M4c command-palette requests (ADR 0094):
 * the recently used actions, kept like the core does (most recent first,
 * counted, at most 20, malformed ids refused). `../../api/mock.ts` routes
 * exactly these requests here.
 */
import type { RecentActions, Request, Response } from "../../api/generated";
import type { MockCore } from "../../api/mockMail";

/** The requests this feature answers. */
export const PALETTE_REQUESTS = [
  "GetRecentActions",
  "RecordActionUsed",
  "ClearRecentActions",
] as const satisfies readonly Request["type"][];

export type PaletteRequest = Extract<Request, { type: (typeof PALETTE_REQUESTS)[number] }>;

export function isPaletteRequest(request: Request): request is PaletteRequest {
  return (PALETTE_REQUESTS as readonly string[]).includes(request.type);
}

/** `nail_api::palette::MAX_RECENT_ACTIONS`. */
export const MAX_RECENT_ACTIONS = 20;

/** `nail_api::palette::is_action_id`. */
export function isActionId(id: string): boolean {
  const chars = [...id].length;
  return chars >= 1 && chars <= 160 && !/[\s\p{Cc}]/u.test(id);
}

/** Mock-core state of this feature. */
export interface PaletteMockState {
  recent: RecentActions;
}

export function createPaletteMockState(): PaletteMockState {
  return { recent: { actions: [] } };
}

/** `RecentActions::record`: front, counted, capped. */
export function recordAction(recent: RecentActions, actionId: string, now: string): RecentActions {
  const previous = recent.actions.find((a) => a.action_id === actionId);
  return {
    actions: [
      { action_id: actionId, last_used_at: now, count: (previous?.count ?? 0) + 1 },
      ...recent.actions.filter((a) => a.action_id !== actionId),
    ].slice(0, MAX_RECENT_ACTIONS),
  };
}

export function answerPalette(core: MockCore, request: PaletteRequest): Response {
  const state = core.data.features.palette;
  switch (request.type) {
    case "GetRecentActions":
      break;
    case "RecordActionUsed":
      if (!isActionId(request.action_id)) {
        return {
          type: "Error",
          code: "invalid_request",
          message: "not an action id",
          localized: { id: "palette-invalid-action" },
        };
      }
      state.recent = recordAction(state.recent, request.action_id, new Date().toISOString());
      break;
    case "ClearRecentActions":
      state.recent = { actions: [] };
      break;
  }
  return { type: "RecentActions", recent: structuredClone(state.recent) };
}
