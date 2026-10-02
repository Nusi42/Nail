/**
 * The mock core's answers to the M4c recovery-key and smartcard requests
 * (ADR 0094), used by `MockTransport` (`../../api/mock.ts` routes exactly
 * these requests here).
 *
 * Contract stage (WP120), like the core: the smartcard settings are stored
 * and validated; listing tokens, using their keys, PIN answers and the
 * recovery key answer `unsupported` (`m4c-not-ready`). WP124 simulates the
 * recovery key, tokens and PIN prompts (`PinRequested`) here.
 */
import type { Request, Response, SmartcardSettings } from "../../api/generated";
import type { MockCore } from "../../api/mockMail";

/** The requests this file answers. */
export const VAULT_REQUESTS = [
  "CreateRecoveryKey",
  "VerifyRecoveryKey",
  "RestoreVaultKey",
  "RemoveRecoveryKey",
  "GetSmartcardSettings",
  "SetSmartcardSettings",
  "ListSmartcards",
  "UseSmartcardKey",
  "AnswerPinPrompt",
] as const satisfies readonly Request["type"][];

export type VaultRequest = Extract<Request, { type: (typeof VAULT_REQUESTS)[number] }>;

export function isVaultRequest(request: Request): request is VaultRequest {
  return (VAULT_REQUESTS as readonly string[]).includes(request.type);
}

/** Mock-core state (kept on `data.features.vault`). */
export interface VaultMockState {
  smartcards: SmartcardSettings;
}

export function createVaultMockState(): VaultMockState {
  return { smartcards: { modules: [], pin_cache_minutes: 0 } };
}

/** `SmartcardSettings::issues` (the first one), as a Fluent id. */
export function smartcardSettingsIssue(settings: SmartcardSettings): string | null {
  const absolute = (p: string) => /^([a-zA-Z]:[\\/]|\\\\|\/)/.test(p);
  if (settings.modules.some((m) => !absolute(m.path))) {
    return "smartcard-settings-issue-relative-path";
  }
  const paths = settings.modules.map((m) => m.path.toLowerCase());
  if (new Set(paths).size !== paths.length) return "smartcard-settings-issue-duplicate-module";
  if (settings.pin_cache_minutes > 480) return "smartcard-settings-issue-cache-too-long";
  return null;
}

export function answerVault(core: MockCore, request: VaultRequest): Response {
  const state = core.data.features.vault;
  switch (request.type) {
    case "GetSmartcardSettings":
      return { type: "SmartcardSettings", settings: structuredClone(state.smartcards) };
    case "SetSmartcardSettings": {
      const issue = smartcardSettingsIssue(request.settings);
      if (issue !== null) {
        return { type: "Error", code: "invalid_request", message: issue, localized: { id: issue } };
      }
      state.smartcards = structuredClone(request.settings);
      return { type: "SmartcardSettings", settings: structuredClone(state.smartcards) };
    }
    default:
      return {
        type: "Error",
        code: "unsupported",
        message: `${request.type} is not available yet`,
        localized: { id: "m4c-not-ready" },
      };
  }
}
