/**
 * The mock core's answers to the M2a security requests (ADR 0029), used by
 * `MockTransport` (`../../api/mock.ts` routes exactly these requests here).
 * State lives in `data.features.security`. Like the core after WP30: the
 * security settings and the vault status work, every other request answers
 * `unsupported` until WP34 / WP35 implement them.
 */
import type { Request, Response, SecuritySettings, VaultStatus } from "../../api/generated";
import { errorResponse, type MockCore } from "../../api/mockMail";

/** The requests this feature answers. */
export const SECURITY_REQUESTS = [
  "ListKeys",
  "GetKey",
  "ImportKeys",
  "ImportPkcs12",
  "ExportPublicKey",
  "GenerateKey",
  "DeleteKey",
  "SetKeyTrust",
  "UnlockKey",
  "LookupKeys",
  "ListWindowsCertificates",
  "ImportWindowsCertificate",
  "CheckRecipientKeys",
  "SendAutocryptSetup",
  "ImportAutocryptSetup",
  "GetSecuritySettings",
  "UpdateSecuritySettings",
  "GetVaultStatus",
  "SetVaultEncryption",
] as const satisfies readonly Request["type"][];

export type SecurityRequest = Extract<Request, { type: (typeof SECURITY_REQUESTS)[number] }>;

export function isSecurityRequest(request: Request): request is SecurityRequest {
  return (SECURITY_REQUESTS as readonly string[]).includes(request.type);
}

/** Mock-core state of this feature. */
export interface SecurityMockState {
  settings: SecuritySettings;
  vault: VaultStatus;
}

/** Defaults as the core has them (`SecuritySettings::default`, plain vault). */
export function createSecurityMockState(): SecurityMockState {
  return {
    settings: {
      index_decrypted: [],
      key_lookup: "on-demand",
      lookup_sources: ["wkd", "keys-openpgp-org"],
    },
    vault: {
      state: "plain",
      phase: null,
      done: 0,
      total: 0,
      key_in_keychain: false,
      error: null,
      message: null,
    },
  };
}

function state(core: MockCore): SecurityMockState {
  return core.data.features.security;
}

export function answerSecurity(core: MockCore, request: SecurityRequest): Response {
  const s = state(core);
  switch (request.type) {
    case "GetSecuritySettings":
      return { type: "SecuritySettings", settings: structuredClone(s.settings) };
    case "UpdateSecuritySettings": {
      const known = new Set(core.data.accounts.map((a) => a.id));
      const unknown = request.settings.index_decrypted.find((id) => !known.has(id));
      if (unknown !== undefined) {
        return errorResponse("invalid_request", `${unknown} is not a real account`);
      }
      s.settings = {
        index_decrypted: [...new Set(request.settings.index_decrypted)].sort(),
        key_lookup: request.settings.key_lookup,
        lookup_sources: [...new Set(request.settings.lookup_sources)],
      };
      core.emit({ type: "SecuritySettingsChanged" });
      return { type: "SecuritySettings", settings: structuredClone(s.settings) };
    }
    case "GetVaultStatus":
      return { type: "VaultStatus", status: structuredClone(s.vault) };
    default:
      return errorResponse("unsupported", `${request.type} is not available yet (M2a)`);
  }
}
