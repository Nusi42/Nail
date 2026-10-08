/**
 * The mock core's answers to the M2c Sieve and vacation requests
 * (ADR 0043), used by `MockTransport` (`../../api/mock.ts` routes exactly
 * these requests here). State lives in `data.features.sieve`.
 *
 * Behaves like the core once WP55 wires `nail-sieve` (ADR 0043): the state
 * per account comes from its `sieve` settings (IMAP accounts only);
 * vacation settings are validated and stored; `SyncSieve` "uploads" (state
 * `in-sync`, the usual extensions), `PreviewSieveScript` returns a small
 * stand-in script. Placements are simplified: arrival rules run on the
 * server, the others in Nail (the real compiler is `nail-sieve`, WP51).
 * `data.features.sieve.failWith` makes uploads fail.
 */
import {
  VACATION_ISSUE_MESSAGE_IDS,
  type AccountConfig,
  type Request,
  type Response,
  type RuleSieveStatus,
  type SieveAccountStatus,
  type VacationSettings,
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
  /** Last upload per account (RFC 3339). */
  uploaded: Record<string, string>;
  /** Makes the next uploads fail with this error code. */
  failWith: "network" | "rejected" | null;
}

export function createSieveMockState(): SieveMockState {
  return { vacation: {}, uploaded: {}, failWith: null };
}

/** Extensions the mock server announces. */
export const MOCK_EXTENSIONS = ["fileinto", "imap4flags", "vacation", "date", "regex", "variables"];

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

function placements(core: MockCore, account: AccountConfig): RuleSieveStatus[] {
  return core.data.features.rules.rules
    .filter(
      (r) =>
        r.enabled &&
        (r.scope.length === 0 ||
          r.scope.some((s) => s.type === "folder" || s.account_id === account.id)),
    )
    .map((r) => ({
      rule_id: r.id,
      placement: r.triggers.some((t) => t.type === "arrival") ? "server" : "client",
      reasons: [],
    }));
}

function status(core: MockCore, account: AccountConfig): SieveAccountStatus {
  const s = state(core);
  const sieve = account.sieve;
  const enabled = sieve !== undefined && sieve.enabled !== false;
  const uploaded = s.uploaded[account.id] ?? null;
  const failed = enabled && s.failWith !== null;
  return {
    account_id: account.id,
    state: !enabled ? "disabled" : failed ? "failed" : uploaded ? "in-sync" : "pending",
    script_name: sieve?.script_name ?? "nail",
    uploaded_at: uploaded,
    extensions: uploaded ? [...MOCK_EXTENSIONS] : [],
    other_scripts: uploaded ? ["roundcube"] : [],
    rules: enabled ? placements(core, account) : [],
    vacation_active: uploaded !== null && (s.vacation[account.id]?.enabled ?? false),
    error: failed ? s.failWith : null,
    message: failed ? "upload failed" : null,
  };
}

function script(core: MockCore, account: AccountConfig): string {
  const vacation = state(core).vacation[account.id];
  const lines = [`# Nail rules for ${account.name}`, 'require ["fileinto", "vacation"];'];
  if (vacation?.enabled) {
    lines.push(`vacation :days ${String(vacation.days)} ${JSON.stringify(vacation.body)};`);
  }
  return `${lines.join("\n")}\n`;
}

export function answerSieve(core: MockCore, request: SieveRequest): Response {
  const s = state(core);
  switch (request.type) {
    case "GetSieveStatus": {
      if (request.account_id === null) {
        const accounts = core.data.accountsConfig.accounts.filter((a) => a.sieve !== undefined);
        return { type: "SieveStatus", accounts: accounts.map((a) => status(core, a)) };
      }
      const account = imapAccount(core, request.account_id);
      if ("type" in account) return account;
      return { type: "SieveStatus", accounts: [status(core, account)] };
    }
    case "SyncSieve": {
      const account = sieveAccount(core, request.account_id);
      if ("type" in account) return account;
      if (account.sieve?.enabled !== false && s.failWith === null) {
        s.uploaded[account.id] = new Date().toISOString();
      }
      core.emit({ type: "SieveStatusChanged", account_id: account.id });
      return { type: "SieveStatus", accounts: [status(core, account)] };
    }
    case "PreviewSieveScript": {
      const account = sieveAccount(core, request.account_id);
      if ("type" in account) return account;
      return {
        type: "SieveScript",
        account_id: account.id,
        script: script(core, account),
        rules: placements(core, account),
      };
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
          message: `invalid vacation settings (${VACATION_ISSUE_MESSAGE_IDS[issue]})`,
          localized: { id: VACATION_ISSUE_MESSAGE_IDS[issue] },
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
