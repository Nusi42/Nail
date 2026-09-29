/**
 * The mock core's answers to the M2c Sieve and vacation requests
 * (ADR 0043), used by `MockTransport` (`../../api/mock.ts` routes exactly
 * these requests here). State lives in `data.features.sieve`.
 *
 * Behaves like the core after WP50: the state per account comes from its
 * `sieve` settings (IMAP accounts only); vacation settings are validated
 * and stored; compiling and uploading (`SyncSieve`, `PreviewSieveScript`)
 * answer `unsupported` until WP55 wires `nail-sieve`. WP54 may extend it.
 */
import type {
  AccountConfig,
  Request,
  Response,
  SieveAccountStatus,
  VacationSettings,
} from "../../api/generated";
import { errorResponse, type MockCore } from "../../api/mockMail";
import { defaultVacation, vacationIssues } from "./model";

/** The requests this feature answers. */
export const SIEVE_REQUESTS = [
  "GetSieveStatus",
  "SyncSieve",
  "PreviewSieveScript",
  "GetVacation",
  "UpdateVacation",
] as const satisfies readonly Request["type"][];

export type SieveRequest = Extract<Request, { type: (typeof SIEVE_REQUESTS)[number] }>;

export function isSieveRequest(request: Request): request is SieveRequest {
  return (SIEVE_REQUESTS as readonly string[]).includes(request.type);
}

/** Mock-core state of this feature. */
export interface SieveMockState {
  /** Vacation settings by account id (absent = the defaults). */
  vacation: Record<string, VacationSettings>;
}

export function createSieveMockState(): SieveMockState {
  return { vacation: {} };
}

function state(core: MockCore): SieveMockState {
  return core.data.features.sieve;
}

/** A configured IMAP account, or the error the core answers. */
function imapAccount(core: MockCore, id: string): AccountConfig | Response {
  const account = core.data.accountsConfig.accounts.find((a) => a.id === id);
  if (!account) return errorResponse("not_found", `account ${id}`);
  if (account.incoming.protocol !== "imap") {
    return errorResponse("unsupported", `account ${id} is not an IMAP account`);
  }
  return account;
}

function sieveAccount(core: MockCore, id: string): AccountConfig | Response {
  const account = imapAccount(core, id);
  if ("type" in account) return account;
  if (!account.sieve) {
    return errorResponse("unsupported", `account ${id} has no ManageSieve settings`);
  }
  return account;
}

function status(account: AccountConfig): SieveAccountStatus {
  const sieve = account.sieve;
  const enabled = sieve !== undefined && sieve.enabled !== false;
  return {
    account_id: account.id,
    state: enabled ? "pending" : "disabled",
    script_name: sieve?.script_name ?? "nail",
    uploaded_at: null,
    extensions: [],
    other_scripts: [],
    rules: [],
    vacation_active: false,
    error: null,
    message: null,
  };
}

export function answerSieve(core: MockCore, request: SieveRequest): Response {
  const s = state(core);
  switch (request.type) {
    case "GetSieveStatus": {
      if (request.account_id === null) {
        const accounts = core.data.accountsConfig.accounts.filter((a) => a.sieve !== undefined);
        return { type: "SieveStatus", accounts: accounts.map(status) };
      }
      const account = imapAccount(core, request.account_id);
      if ("type" in account) return account;
      return { type: "SieveStatus", accounts: [status(account)] };
    }
    case "SyncSieve":
    case "PreviewSieveScript": {
      const account = sieveAccount(core, request.account_id);
      if ("type" in account) return account;
      return errorResponse("unsupported", "Sieve uploads are not wired yet");
    }
    case "GetVacation": {
      const account = imapAccount(core, request.account_id);
      if ("type" in account) return account;
      const settings = s.vacation[account.id] ?? defaultVacation();
      return { type: "Vacation", account_id: account.id, settings: structuredClone(settings) };
    }
    case "UpdateVacation": {
      const account = sieveAccount(core, request.account_id);
      if ("type" in account) return account;
      const issue = vacationIssues(request.settings)[0];
      if (issue !== undefined) {
        return {
          type: "Error",
          code: "invalid_request",
          message: `invalid vacation settings (vacation-issue-${issue})`,
          localized: { id: `vacation-issue-${issue}` },
        };
      }
      s.vacation[account.id] = structuredClone(request.settings);
      core.emit({ type: "SieveStatusChanged", account_id: account.id });
      return {
        type: "Vacation",
        account_id: account.id,
        settings: structuredClone(request.settings),
      };
    }
  }
}
